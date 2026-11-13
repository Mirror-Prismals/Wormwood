"""Owned Carbon menu descriptions; guest menu pointers never reach Cocoa."""
from macho import CompatibilityError

SYMBOLS = ('_GetMenuHeight', '_CalcMenuSize', '_UniqueID', '_NewMenu', '_DisposeMenu', '_DeleteMenu', '_InsertMenu',
           '_InsertMenuItemTextWithCFString', '_AppendMenuItemTextWithCFString', '_AppendMenu', '_CheckMenuItem', '_EnableMenuItem', '_DisableMenuItem',
           '_SetMenuItemHierarchicalMenu', '_SetMenuItemText', '_SetItemCmd',
           '_SetMenuItemKeyGlyph', '_SetMenuItemModifiers', '_PopUpMenuSelect')


class Menu32:
    def __init__(self, runtime):
        self.runtime = runtime
        self.records = {}
        self.ids = {}
        self.next_token = 0x76200000
        self.next_id = 256
        self.popup = None

    def describe(self, token, ancestors=()):
        if token in ancestors or len(ancestors) >= 8:
            raise CompatibilityError('Cyclic or excessive Carbon menu nesting')
        menu = self.records[token]
        result = []
        for index, item in enumerate(menu['items'], 1):
            entry = {key: item[key] for key in ('title', 'enabled', 'checked', 'separator')}
            entry['selection'] = (menu['id'] << 16) | index
            if item['submenu']:
                entry['children'] = self.describe(item['submenu'], (*ancestors, token))
            result.append(entry)
        return result

    def pascal(self, pointer):
        size = self.runtime.read(pointer, 1)[0]
        return self.runtime.read(pointer + 1, size).decode('mac_roman')

    def invoke(self, name, arg):
        if name == '_UniqueID':
            if arg(0) != int.from_bytes(b'MENU', 'big'):
                raise CompatibilityError('UniqueID resource type is not implemented')
            while self.next_id in self.ids:
                self.next_id += 1
            if self.next_id >= 32767:
                raise CompatibilityError('Carbon menu ID limit exceeded')
            result = self.next_id
            self.next_id += 1
            return result
        if name == '_NewMenu':
            ident = arg(0) & 0xffff
            if ident in self.ids or len(self.records) >= 256 or self.next_token >= 0x76300000:
                raise CompatibilityError('Invalid or excessive Carbon menus')
            token = self.next_token
            self.next_token += 16
            self.records[token] = dict(id=ident, title=self.pascal(arg(1)), items=[], inserted=False)
            self.ids[ident] = token
            return token
        if name == '_DeleteMenu':
            token = self.ids.get(arg(0) & 0xffff)
            if token is not None:
                self.records[token]['inserted'] = False
            return 0
        token = arg(0)
        menu = self.records.get(token)
        if menu is None:
            raise CompatibilityError('Unknown Carbon menu')
        if name == '_GetMenuHeight':
            return min(32767, max(1, len(menu['items'])) * 20)
        if name == '_CalcMenuSize':
            return 0  # Host presentation computes geometry from the menu description.
        if name == '_DisposeMenu':
            del self.ids[menu['id']]
            del self.records[token]
            return 0
        if name == '_InsertMenu':
            menu['inserted'] = True
            return 0
        if name in ('_InsertMenuItemTextWithCFString', '_AppendMenuItemTextWithCFString'):
            if len(menu['items']) >= 1024:
                raise CompatibilityError('Carbon menu item limit exceeded')
            insert = name == '_InsertMenuItemTextWithCFString'
            after = arg(2) if insert else len(menu['items'])
            after = min(after & 0xffff, len(menu['items']))
            attributes = arg(3) if insert else arg(2)
            title = self.runtime.cf.constant_string(arg(1))
            item = dict(title=title, enabled=not bool(attributes & 1), checked=False,
                        separator=bool(attributes & 64), submenu=0,
                        command=arg(4) if insert else arg(3))
            menu['items'].insert(after, item)
            if not insert and arg(4):
                import struct
                self.runtime.cpu.mem_write(arg(4), struct.pack('<H', after + 1))
            return 0
        if name == '_AppendMenu':
            entries = self.pascal(arg(1)).split(';')
            if len(menu['items']) + len(entries) > 1024:
                raise CompatibilityError('Carbon menu item limit exceeded')
            for text in entries:
                enabled = not text.startswith('(')
                if not enabled:
                    text = text[1:]
                menu['items'].append(dict(title=text, enabled=enabled, checked=False,
                                          separator=text == '-', submenu=0))
            return 0
        if name == '_PopUpMenuSelect':
            if self.popup is None:
                raise CompatibilityError('Carbon popup presentation is not connected')
            return self.popup(token, arg(1), arg(2), arg(3))
        index = arg(1) & 0xffff
        if not 1 <= index <= len(menu['items']):
            raise CompatibilityError(f'Invalid Carbon menu item index: {name} {index}/{len(menu["items"])}')
        item = menu['items'][index - 1]
        if name == '_SetMenuItemHierarchicalMenu':
            if arg(2) and arg(2) not in self.records:
                raise CompatibilityError('Unknown Carbon submenu')
            item['submenu'] = arg(2)
        elif name == '_SetMenuItemText':
            item['title'] = self.pascal(arg(2))
        elif name == '_CheckMenuItem':
            item['checked'] = bool(arg(2) & 255)
        elif name in ('_EnableMenuItem', '_DisableMenuItem'):
            item['enabled'] = name == '_EnableMenuItem'
        else:
            item[name[1:]] = arg(2)
        return 0
