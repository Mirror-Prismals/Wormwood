//! Minimal GLFW native window connection. Windows stay hidden until Go.
use libloading::Library;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    ffi::{CStr, c_char, c_int, c_void},
    ptr,
    rc::{Rc, Weak},
};
type P = *mut c_void;
fn workstation_overview(shift:bool,command:bool)->bool {shift&&!command}
type RefreshCallback = Option<unsafe extern "C" fn(P)>;
type IconifyCallback = Option<unsafe extern "C" fn(P, c_int)>;
type ScrollCallback = Option<unsafe extern "C" fn(P, f64, f64)>;
type KeyCallback = Option<unsafe extern "C" fn(P, c_int, c_int, c_int, c_int)>;
type MouseCallback = Option<unsafe extern "C" fn(P,c_int,c_int,c_int)>;
type CursorPosition = unsafe extern "C" fn(P,*mut f64,*mut f64);
#[derive(Clone,Copy)]struct TransportPointer {point:[f64;2],down:bool}
type FocusCallback = Option<unsafe extern "C" fn(P, c_int)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PianoPress { key:c_int, mods:c_int }
#[derive(Default)]
struct WorkstationKeys {
    export:bool, transport_toggle:bool, transport_toggle_timeline:bool, midi_cancelled:bool,
    split_clip:bool, freeze_track:bool, preamp_switch:bool,
    range_create:bool, range_chord_down:bool, m_down:bool,
    loop_range:bool, loop_chord_down:bool, l_down:bool,
    mouse:VecDeque<TransportPointer>, mouse_cancelled:bool, cursor_position:Option<CursorPosition>,
    piano:VecDeque<PianoPress>,
    safety:super::input_safety::BufferedKeys,
    sfx_key_press:bool,
    browser_keys:Vec<[i32;2]>,
    relevant_presses:[u64;6],
}
impl WorkstationKeys {
    fn record_mouse(&mut self,point:[f64;2],down:bool){
        if self.mouse_cancelled{return;}
        if self.mouse.len()>=16 || !point.iter().all(|v|v.is_finite()) {self.mouse.clear();self.mouse_cancelled=true;return;}
        self.mouse.push_back(TransportPointer{point,down});
    }
    fn take_mouse(&mut self)->(Vec<TransportPointer>,bool){
        (self.mouse.drain(..).collect(),std::mem::take(&mut self.mouse_cancelled))
    }
    fn record(&mut self,key:c_int,action:c_int,mods:c_int) {
        self.safety.record(key,action,mods,cfg!(target_os="macos"));
        if matches!(action,1|2)&&self.browser_keys.len()<128 {self.browser_keys.push([key,mods]);}
        // Track the complete rising chord, including modifiers pressed after M.
        if key==77 {self.m_down=action!=0;}
        let command_mask=if cfg!(target_os="macos"){0x0008}else{0x0002};
        let chord=self.m_down && mods&1!=0 && mods&command_mask!=0;
        if chord && !self.range_chord_down && action==1 {self.range_create=true;}
        self.range_chord_down=chord;
        if key==76 {self.l_down=action!=0;}
        let loop_command=if cfg!(target_os="windows"){0x0002}else{0x0008};
        let loop_chord=self.l_down && mods&1==0 && mods&loop_command!=0;
        if loop_chord && !self.loop_chord_down && action==1 {self.loop_range=true;}
        self.loop_chord_down=loop_chord;
        if key==69 && action==1 && mods&command_mask!=0 && mods&5==0 {self.split_clip=true;}
        if key==70 && action==1 && mods&15==0{self.freeze_track=true;}
        if key==90 && action==1 && mods&15==0{self.preamp_switch=true;}
        if action!=1 {return;} // GLFW_PRESS only; held/repeat is not a new command.
        self.sfx_key_press=true;
        if key==32 {
            self.transport_toggle=true;
            // Cardinal timeline excludes Alt and its platform edit modifier;
            // the ordinary piano roll accepts Space independently of those.
            let timeline_command=if cfg!(target_os="windows"){0x0002}else{0x0008};
            self.transport_toggle_timeline|=mods&(timeline_command|0x0004)==0;
        }
        if let Some(i)=[265,264,263,262,256,81].iter().position(|k|*k==key) {
            self.relevant_presses[i]=self.relevant_presses[i].saturating_add(1);
        }
        if key==69 && mods&0x0001!=0 && mods&(0x0002|0x0008)!=0 {self.export=true;}
        if key==257 && self.piano.len()<8 {
            self.piano.push_back(PianoPress{key,mods});
        }
    }
    fn clear(&mut self) {self.browser_keys.clear();self.preamp_switch=false;self.freeze_track=false;self.split_clip=false;self.sfx_key_press=false;self.midi_cancelled=true;self.mouse.clear();self.mouse_cancelled=true;self.export=false;self.transport_toggle=false;self.transport_toggle_timeline=false;self.piano.clear();self.range_create=false;self.range_chord_down=false;self.m_down=false;self.loop_range=false;self.loop_chord_down=false;self.l_down=false;self.safety.blur();}
    fn take(&mut self,focused:bool)->(bool,Option<PianoPress>) {
        if !focused {self.clear();return (false,None);}
        (std::mem::take(&mut self.export),self.piano.pop_front())
    }
}

// GLFW dispatches these callbacks on the window's thread. Weak flags avoid a
// movable Rust object pointer or shared native user-pointer ownership.
thread_local! {
    static GLFW_USERS: Cell<usize> = const { Cell::new(0) };
    static REFRESH_FLAGS: RefCell<BTreeMap<usize, Weak<Cell<bool>>>> = RefCell::new(BTreeMap::new());
    static WORKSTATION_SCROLL: RefCell<BTreeMap<usize, Weak<Cell<[f64;2]>>>> = RefCell::new(BTreeMap::new());
    // Keep short commands even when their releases arrive in the same poll.
    // Enter note presses retain their order; clip opening is a pointer gesture.
    static WORKSTATION_KEYS: RefCell<BTreeMap<usize, Weak<RefCell<WorkstationKeys>>>> = RefCell::new(BTreeMap::new());
}
unsafe extern "C" fn workstation_key(handle:P,key:c_int,_scan:c_int,action:c_int,mods:c_int) {
    WORKSTATION_KEYS.with(|map| {
        if let Some(value)=map.borrow().get(&(handle as usize)).and_then(Weak::upgrade) {
            value.borrow_mut().record(key,action,mods);
        }
    });
}
unsafe extern "C" fn workstation_mouse(handle:P,button:c_int,action:c_int,_mods:c_int) {
    if button!=0 || !matches!(action,0|1){return;}
    WORKSTATION_KEYS.with(|map| {
        if let Some(value)=map.borrow().get(&(handle as usize)).and_then(Weak::upgrade) {
            let mut keys=value.borrow_mut();
            if let Some(cursor)=keys.cursor_position {let(mut x,mut y)=(0.,0.);cursor(handle,&mut x,&mut y);keys.record_mouse([x,y],action==1);}
        }
    });
}
unsafe extern "C" fn workstation_focus(handle:P,focused:c_int) {
    if focused!=0 {return;}
    WORKSTATION_KEYS.with(|map| {
        if let Some(value)=map.borrow().get(&(handle as usize)).and_then(Weak::upgrade) {
            value.borrow_mut().clear();
        }
    });
    WORKSTATION_SCROLL.with(|map| {
        if let Some(value)=map.borrow().get(&(handle as usize)).and_then(Weak::upgrade) {value.set([0.;2]);}
    });
}
unsafe extern "C" fn workstation_scroll(handle:P,x:f64,y:f64) {
    if !x.is_finite() || !y.is_finite() {return;}
    WORKSTATION_SCROLL.with(|map| {
        if let Some(value)=map.borrow().get(&(handle as usize)).and_then(Weak::upgrade) {
            let old=value.get();value.set([(old[0]+x).clamp(-100.,100.),(old[1]+y).clamp(-100.,100.)]);
        }
    });
}
unsafe extern "C" fn refresh(handle: P) {
    REFRESH_FLAGS.with(|flags| {
        if let Some(flag) = flags
            .borrow()
            .get(&(handle as usize))
            .and_then(Weak::upgrade)
        {
            flag.set(true);
        }
    });
}
unsafe extern "C" fn iconify(handle: P, _: c_int) {
    unsafe {
        refresh(handle);
    }
}
#[repr(C)]
struct VideoMode {
    width: c_int,
    height: c_int,
    red_bits: c_int,
    green_bits: c_int,
    blue_bits: c_int,
    refresh_rate: c_int,
}

// Matches vkGetInstanceProcAddr as generated from the installed headers on
// macOS arm64 and Windows x64 (where __stdcall lowers to the C ABI).
pub(super) type VulkanLoader = Option<
    unsafe extern "C" fn(super::vk::VkInstance, *const c_char) -> super::vk::PFN_vkVoidFunction,
>;
// Windows may coexist while the hidden workstation waits for first entry.
// Release GLFW only after the final window, including constructor failures.
struct GlfwLease(unsafe extern "C" fn());
impl Drop for GlfwLease {
    fn drop(&mut self) { GLFW_USERS.with(|users| {
        let remaining=users.get()-1;users.set(remaining);
        if remaining==0 {unsafe{(self.0)();}}
    }); }
}
pub(super) struct Window {
    pub handle: P,
    _glfw: GlfwLease, // drops before the library holding its function pointer
    lib: Library,
    destroy: unsafe extern "C" fn(P),
    deferred_monitor: Cell<Option<(P,i32,i32)>>,
    set_monitor: unsafe extern "C" fn(P,P,c_int,c_int,c_int,c_int,c_int),
    poll: unsafe extern "C" fn(),
    should_close: unsafe extern "C" fn(P) -> c_int,
    show_window: unsafe extern "C" fn(P),
    captured: bool,
    virtual_drag: bool,
    pointer_mode: Option<bool>,
    cursor: [f64; 2],
    pub swap: unsafe extern "C" fn(P),
    framebuffer: unsafe extern "C" fn(P, *mut c_int, *mut c_int),
    logical_backing: bool,
    shown: Cell<bool>,
    display_cadence:Cell<Option<(std::time::Instant,crate::workstation::frame_cadence::Display)>>,
    refresh: Rc<Cell<bool>>,
    workstation_scroll: Rc<Cell<[f64;2]>>,
    workstation_keys: Rc<RefCell<WorkstationKeys>>,
    sampled_escape:Cell<bool>,
    escape_press:Cell<bool>,
    preview_controls:bool,
    preview_epoch:std::time::Instant,
    preview_rate:RefCell<super::draw_rate::DrawRate>,
}
impl Window {
    pub unsafe fn new(opengl: bool, title: &CStr, workstation: bool) -> Result<Self, String> {
        Self::with_loader(opengl, title, None, workstation)
    }
    pub unsafe fn with_loader(
        opengl: bool,
        title: &CStr,
        loader: VulkanLoader,
        workstation: bool,
    ) -> Result<Self, String> {
        Self::with_options(opengl,title,loader,workstation,false)
    }
    pub unsafe fn prewarm(title:&CStr)->Result<Self,String>{
        Self::with_options(false,title,None,true,true)
    }
    unsafe fn with_options(opengl:bool,title:&CStr,loader:VulkanLoader,workstation:bool,defer_fullscreen:bool)->Result<Self,String>{
        let path = std::env::var("WORMWOOD_GLFW_LIBRARY").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "/opt/homebrew/lib/libglfw.dylib"
            } else if cfg!(target_os = "windows") {
                "glfw3.dll"
            } else {
                "libglfw.so.3"
            }
            .into()
        });
        let lib = Library::new(&path).map_err(|e| format!("GLFW {path}: {e}"))?;
        let init = *lib
            .get::<unsafe extern "C" fn() -> c_int>(b"glfwInit\0")
            .map_err(|e| e.to_string())?;
        let terminate = *lib
            .get::<unsafe extern "C" fn()>(b"glfwTerminate\0")
            .map_err(|e| e.to_string())?;
        let hint = *lib
            .get::<unsafe extern "C" fn(c_int, c_int)>(b"glfwWindowHint\0")
            .map_err(|e| e.to_string())?;
        let init_hint = *lib
            .get::<unsafe extern "C" fn(c_int, c_int)>(b"glfwInitHint\0")
            .map_err(|e| e.to_string())?;
        let create = *lib
            .get::<unsafe extern "C" fn(c_int, c_int, *const c_char, P, P) -> P>(
                b"glfwCreateWindow\0",
            )
            .map_err(|e| e.to_string())?;
        let destroy = *lib
            .get::<unsafe extern "C" fn(P)>(b"glfwDestroyWindow\0")
            .map_err(|e| e.to_string())?;
        let poll = *lib
            .get::<unsafe extern "C" fn()>(b"glfwPollEvents\0")
            .map_err(|e| e.to_string())?;
        let should_close = *lib
            .get::<unsafe extern "C" fn(P) -> c_int>(b"glfwWindowShouldClose\0")
            .map_err(|e| e.to_string())?;
        let show_window = *lib
            .get::<unsafe extern "C" fn(P)>(b"glfwShowWindow\0")
            .map_err(|e| e.to_string())?;
        let swap = *lib
            .get::<unsafe extern "C" fn(P)>(b"glfwSwapBuffers\0")
            .map_err(|e| e.to_string())?;
        let framebuffer = *lib
            .get::<unsafe extern "C" fn(P, *mut c_int, *mut c_int)>(b"glfwGetFramebufferSize\0")
            .map_err(|e| e.to_string())?;
        // GLFW snapshots init hints during glfwInit. Supply/reset the Vulkan
        // loader before that snapshot, not after creating a window.
        let init_loader = *lib
            .get::<unsafe extern "C" fn(VulkanLoader)>(b"glfwInitVulkanLoader\0")
            .map_err(|e| e.to_string())?;
        init_loader(loader);
        // In GLFW 3.4 this also enables NSApplicationActivationPolicyRegular.
        // Disabling it leaves an unbundled workstation unable to activate; the
        // focus gate then correctly defers every draw. Preserve the game policy.
        init_hint(0x00051002, i32::from(workstation)); // GLFW_COCOA_MENUBAR
        if GLFW_USERS.with(|users|users.get()==0) && init() == 0 {
            return Err("GLFW initialization failed".into());
        }
        GLFW_USERS.with(|users|users.set(users.get()+1));
        let glfw=GlfwLease(terminate);
        let set_monitor=*lib.get::<unsafe extern "C" fn(P,P,c_int,c_int,c_int,c_int,c_int)>(b"glfwSetWindowMonitor\0").map_err(|e|e.to_string())?;
        hint(0x00020004, 0); // GLFW_VISIBLE
        hint(0x00020003, 0); // GLFW_RESIZABLE

        // Keep the physical window and Origami point coordinates unchanged.
        // This optical scene renders one pixel per logical point; other scenes
        // retain Retina backing. Native-density comparison is opt-in while its
        // engraving quality and animation cost are being evaluated.
        let game_window = !workstation && std::env::var("WORMWOOD_GAME_WINDOW").as_deref() == Ok("1");
        #[cfg(target_os="macos")]
        let standard_game_pixels = game_window && {
            unsafe extern "C" { fn ww_game_native_pixels() -> bool; }
            !ww_game_native_pixels()
        };
        #[cfg(not(target_os="macos"))]
        let standard_game_pixels = game_window;
        let logical_backing = standard_game_pixels || cfg!(target_os = "macos")
            && workstation
            && std::env::var("WORMWOOD_WORKSTATION_RETINA").as_deref() != Ok("1");
        // Reset the hint explicitly for every creation.
        #[cfg(target_os = "macos")]
        hint(0x00023001, i32::from(!logical_backing)); // GLFW_COCOA_RETINA_FRAMEBUFFER
        #[cfg(not(target_os = "macos"))]
        let _ = workstation;
        if opengl {
            hint(0x00022002, 3);
            hint(0x00022003, 3);
            hint(0x00022006, 1);
            hint(0x00022008, 0x00032001);
        } else {
            hint(0x00022001, 0);
        } // GLFW_CLIENT_API = GLFW_NO_API
        // Standalone DAW defaults to the monitor size; diagnostics may opt out.
        let fullscreen = match std::env::var("WORMWOOD_FULLSCREEN").as_deref() {
            Ok("1") => true,
            Ok("0") => false,
            _ => workstation && cfg!(target_os = "macos")
                && std::env::var_os("WORMWOOD_WORKSTATION_SIZE").is_none(),
        };
        let mut monitor = ptr::null_mut();
        let (mut width, mut height) = if workstation && cfg!(target_os = "macos") { (960, 600) } else if game_window { (960, 600) } else { (640, 480) };
        if workstation && !fullscreen {
            if let Ok(size) = std::env::var("WORMWOOD_WORKSTATION_SIZE") {
                let dimensions = size.split_once('x').and_then(|(w,h)| Some((w.parse::<i32>().ok()?,h.parse::<i32>().ok()?)));
                match dimensions {
                    Some((w,h)) if (320..=2048).contains(&w) && (320..=2048).contains(&h) => { width=w; height=h; }
                    _ => { return Err("WORMWOOD_WORKSTATION_SIZE requires WIDTHxHEIGHT within 320..2048".into()); }
                }
            }
        }
        if fullscreen {
            let primary = *lib
                .get::<unsafe extern "C" fn() -> P>(b"glfwGetPrimaryMonitor\0")
                .map_err(|e| e.to_string())?;
            let mode = *lib
                .get::<unsafe extern "C" fn(P) -> *const VideoMode>(b"glfwGetVideoMode\0")
                .map_err(|e| e.to_string())?;
            monitor = primary();
            if monitor.is_null() {
                return Err("fullscreen monitor unavailable".into());
            }
            let video = mode(monitor);
            if video.is_null() {
                return Err("fullscreen video mode unavailable".into());
            }
            width = (*video).width;
            height = (*video).height;
        }
        let deferred_monitor=if defer_fullscreen&&!monitor.is_null(){Some((monitor,width,height))}else{None};
        let handle = create(width, height, title.as_ptr(), if defer_fullscreen{ptr::null_mut()}else{monitor}, ptr::null_mut());
        if handle.is_null() {
            return Err("GLFW window/context creation failed".into());
        }
        let result = Self {
            handle,
            _glfw:glfw,
            lib,
            destroy,
            deferred_monitor:Cell::new(deferred_monitor),
            set_monitor,
            poll,
            should_close,
            show_window,
            captured: false,
            virtual_drag: false,
            pointer_mode: None,
            cursor: [0., 0.],
            swap,
            framebuffer,
            logical_backing,
            shown: Cell::new(false),
            display_cadence:Cell::new(None),
            refresh: Rc::new(Cell::new(true)),
            workstation_scroll: Rc::new(Cell::new([0.;2])),
            workstation_keys: Rc::new(RefCell::new(WorkstationKeys::default())),
            sampled_escape:Cell::new(false),
            escape_press:Cell::new(false),
            preview_controls:workstation && cfg!(target_os="macos") && crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_PREVIEW_CONTROLS"),
            preview_epoch:std::time::Instant::now(),
            preview_rate:RefCell::new(super::draw_rate::DrawRate::default()),
        };
        // Install for game windows too: their embedded workstation shares the
        // same native window. Each input sample consumes unused command edges.
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().insert(handle as usize,Rc::downgrade(&result.workstation_keys));});
        result.workstation_keys.borrow_mut().cursor_position=Some(result.symbol::<CursorPosition>(b"glfwGetCursorPos\0")?);
        result.symbol::<unsafe extern "C" fn(P,MouseCallback)->MouseCallback>(b"glfwSetMouseButtonCallback\0")?(handle,Some(workstation_mouse));
        result.symbol::<unsafe extern "C" fn(P,KeyCallback)->KeyCallback>(b"glfwSetKeyCallback\0")?(handle,Some(workstation_key));
        result.symbol::<unsafe extern "C" fn(P,FocusCallback)->FocusCallback>(b"glfwSetWindowFocusCallback\0")?(handle,Some(workstation_focus));
        if workstation {
            WORKSTATION_SCROLL.with(|map| {map.borrow_mut().insert(handle as usize,Rc::downgrade(&result.workstation_scroll));});
            result.symbol::<unsafe extern "C" fn(P,ScrollCallback)->ScrollCallback>(b"glfwSetScrollCallback\0")?(handle,Some(workstation_scroll));
            REFRESH_FLAGS.with(|flags| {
                flags
                    .borrow_mut()
                    .insert(handle as usize, Rc::downgrade(&result.refresh));
            });
            result.symbol::<unsafe extern "C" fn(P, RefreshCallback) -> RefreshCallback>(
                b"glfwSetWindowRefreshCallback\0",
            )?(handle, Some(refresh));
            result.symbol::<unsafe extern "C" fn(P, IconifyCallback) -> IconifyCallback>(
                b"glfwSetWindowIconifyCallback\0",
            )?(handle, Some(iconify));
        }
        if opengl {
            let current = result.symbol::<unsafe extern "C" fn(P)>(b"glfwMakeContextCurrent\0")?;
            current(handle);
            result.symbol::<unsafe extern "C" fn(c_int)>(b"glfwSwapInterval\0")?(1);
        }
        Ok(result)
    }
    pub unsafe fn symbol<T: Copy>(&self, name: &[u8]) -> Result<T, String> {
        self.lib
            .get::<T>(name)
            .map(|v| *v)
            .map_err(|e| e.to_string())
    }
    pub unsafe fn dimensions(&self) -> Result<(u32, u32), String> {
        self.available_dimensions()?
            .ok_or_else(|| "invalid framebuffer dimensions".into())
    }
    pub unsafe fn available_dimensions(&self) -> Result<Option<(u32, u32)>, String> {
        // GLFW 3.4's Cocoa framebuffer query converts view bounds to screen
        // backing pixels even when its surface scaling hint is disabled. Our
        // workstation surface explicitly uses one drawable pixel per point.
        if self.logical_backing {
            let Some((width, height)) = self.available_logical_dimensions()? else {
                return Ok(None);
            };
            if width > 8192. || height > 8192. {
                return Err("invalid workstation drawable dimensions".into());
            }
            return Ok(Some((width as u32, height as u32)));
        }
        let (mut w, mut h) = (0, 0);
        (self.framebuffer)(self.handle, &mut w, &mut h);
        if w > 8192 || h > 8192 {
            return Err("invalid framebuffer dimensions".into());
        }
        Ok((w > 0 && h > 0).then_some((w as u32, h as u32)))
    }
    pub unsafe fn logical_dimensions(&self) -> Result<(f64, f64), String> {
        self.available_logical_dimensions()?
            .ok_or_else(|| "invalid logical window dimensions".into())
    }
    pub unsafe fn available_logical_dimensions(&self) -> Result<Option<(f64, f64)>, String> {
        let (mut width, mut height) = (0, 0);
        self.symbol::<unsafe extern "C" fn(P, *mut c_int, *mut c_int)>(b"glfwGetWindowSize\0")?(
            self.handle,
            &mut width,
            &mut height,
        );
        Ok((width > 0 && height > 0).then_some((width as f64, height as f64)))
    }
    pub unsafe fn hide(&self)->Result<(),String>{
        self.symbol::<unsafe extern "C" fn(P,c_int,c_int)>(b"glfwSetWindowAttrib\0")?(self.handle,0x00020006,0); // GLFW_AUTO_ICONIFY
        self.symbol::<unsafe extern "C" fn(P)>(b"glfwHideWindow\0")?(self.handle);
        self.shown.set(false);
        Ok(())
    }
    pub unsafe fn show(&self) {
        if !self.shown.replace(true) {
            if let Some((monitor,width,height))=self.deferred_monitor.take(){
                (self.set_monitor)(self.handle,monitor,0,0,width,height,-1);
            }
            (self.show_window)(self.handle);
            if let Ok(restore)=self.symbol::<unsafe extern "C" fn(P)>(b"glfwRestoreWindow\0"){restore(self.handle);}
            if let Ok(focus)=self.symbol::<unsafe extern "C" fn(P)>(b"glfwFocusWindow\0"){focus(self.handle);}
            #[cfg(target_os="macos")]
            if let Ok(cocoa)=self.symbol::<unsafe extern "C" fn(P)->P>(b"glfwGetCocoaWindow\0") {
                unsafe extern "C" {fn ww_native_window_activate(window:P);}
                ww_native_window_activate(cocoa(self.handle));
            }
            #[cfg(target_os="macos")]
            if self.preview_controls {
                if let Ok(cocoa)=self.symbol::<unsafe extern "C" fn(P)->P>(b"glfwGetCocoaWindow\0") {
                    unsafe extern "C" {fn ww_workstation_controls_show(window:P);}
                    ww_workstation_controls_show(cocoa(self.handle));
                }
            }
        }
    }
    pub unsafe fn preview_rate(&self,drawn:bool,focused:bool) {
        if !self.preview_controls {return;}
        if let Some(rate)=self.preview_rate.borrow_mut().observe(self.preview_epoch.elapsed().as_secs_f64(),drawn) {
            #[cfg(target_os="macos")]
            if let Ok(cocoa)=self.symbol::<unsafe extern "C" fn(P)->P>(b"glfwGetCocoaWindow\0") {
                unsafe extern "C" {fn ww_workstation_controls_rate(window:P,fps:f64,focused:bool);}
                ww_workstation_controls_rate(cocoa(self.handle),rate,focused);
            }
        }
    }
    pub unsafe fn display_cadence(&self)->crate::workstation::frame_cadence::Display {
        if let Some((when,display))=self.display_cadence.get(){if when.elapsed().as_secs_f64()<0.5{return display;}}
        let mut display=crate::workstation::frame_cadence::Display::default();
        #[cfg(target_os="macos")]
        if let Ok(cocoa)=self.symbol::<unsafe extern "C" fn(P)->P>(b"glfwGetCocoaWindow\0") {
            unsafe extern "C" {fn ww_display_cadence(window:P,values:*mut f64,id:*mut u32);}
            let mut values=[0.;4];let mut id=0;
            ww_display_cadence(cocoa(self.handle),values.as_mut_ptr(),&mut id);
            display=crate::workstation::frame_cadence::Display{current_mode_hz:values[0],maximum_hz:values[1],minimum_interval:values[2],maximum_interval:values[3],id,source:"window_NSScreen_and_current_CGDisplayMode"};
        }
        #[cfg(not(target_os="macos"))]
        if let (Ok(monitor),Ok(mode))=(self.symbol::<unsafe extern "C" fn(P)->P>(b"glfwGetWindowMonitor\0"),self.symbol::<unsafe extern "C" fn(P)->*const VideoMode>(b"glfwGetVideoMode\0")) {
            let monitor=monitor(self.handle);
            if !monitor.is_null(){let mode=mode(monitor);if !mode.is_null(){display.current_mode_hz=(*mode).refresh_rate as f64;display.source="GLFW_current_fullscreen_mode";}}
        }
        self.display_cadence.set(Some((std::time::Instant::now(),display)));display
    }
    pub fn take_refresh(&self) -> bool {
        self.refresh.replace(false)
    }
    pub unsafe fn iconified(&self) -> Result<bool, String> {
        Ok(
            self.symbol::<unsafe extern "C" fn(P, c_int) -> c_int>(b"glfwGetWindowAttrib\0")?(
                self.handle,
                0x00020002, // GLFW_ICONIFIED
            ) != 0,
        )
    }
    pub unsafe fn request_close(&self) -> Result<(), String> {
        self.symbol::<unsafe extern "C" fn(P, c_int)>(b"glfwSetWindowShouldClose\0")?(
            self.handle,
            1,
        );
        Ok(())
    }
    pub unsafe fn closed(&self) -> bool {
        (self.poll)();
        (self.should_close)(self.handle) != 0
    }
    pub unsafe fn input_mode(&mut self, pointer: bool) -> Result<(), String> {
        self.pointer_mode = Some(pointer);
        self.captured = false;
        self.virtual_drag = false;
        self.symbol::<unsafe extern "C" fn(P, c_int, c_int)>(b"glfwSetInputMode\0")?(
            self.handle,
            0x00033001,
            if pointer { 0x00034001 } else { 0x00034003 },
        );
        Ok(())
    }
    #[cfg(target_os="macos")]
    pub unsafe fn pause_menu(&self, visible:bool, state:&serde_json::Value)->Result<(),String> {
        unsafe extern "C" {fn ww_pause_menu(window:P,visible:bool,flags:u32,minutes:i32);}
        let cocoa=self.symbol::<unsafe extern "C" fn(P)->P>(b"glfwGetCocoaWindow\0")?(self.handle);
        let flags=u32::from(state["cheats"]==true) | (u32::from(state["flying"]==true)<<1) | (u32::from(state["noclip"]==true)<<2) | (u32::from(state["sound_speakers"]==true)<<3) | (u32::from(state["night_vision"]==true)<<4);
        ww_pause_menu(cocoa,visible,flags,state["time_override_minutes"].as_i64().unwrap_or(-1) as i32);
        Ok(())
    }
    #[cfg(target_os="macos")]
    unsafe fn pause_menu_input(&self)->Result<serde_json::Value,String> {
        unsafe extern "C" {fn ww_pause_menu_input(window:P,action:*mut u32,flags:*mut u32,minutes:*mut i32)->bool;}
        let cocoa=self.symbol::<unsafe extern "C" fn(P)->P>(b"glfwGetCocoaWindow\0")?(self.handle);
        let (mut action,mut flags,mut minutes)=(0,0,-1);
        if !ww_pause_menu_input(cocoa,&mut action,&mut flags,&mut minutes) {return Ok(serde_json::Value::Null);}
        Ok(serde_json::json!({"active":true,"resume":action==1,"quit":action==2,"time_override_minutes":minutes,
            "cheats":{"enabled":flags&1!=0,"flight":flags&2!=0,"noclip":flags&4!=0,"sound_speakers":flags&8!=0,"night_vision":flags&16!=0,"sequence":0}}))
    }
    pub unsafe fn flight_input(&mut self) -> Result<serde_json::Value, String> {
        #[allow(unused_mut)] let mut native_actions=0u32;
        #[allow(unused_mut)] let mut native_focus=0u32;
        #[cfg(target_os="macos")]
        if self.preview_controls {
            unsafe extern "C" {fn ww_workstation_controls_take(window:P,status:*mut u32)->u32;}
            let cocoa=self.symbol::<unsafe extern "C" fn(P)->P>(b"glfwGetCocoaWindow\0")?;
            native_actions=ww_workstation_controls_take(cocoa(self.handle),&mut native_focus);
        }
        let key = self.symbol::<unsafe extern "C" fn(P, c_int) -> c_int>(b"glfwGetKey\0")?;
        let mouse =
            self.symbol::<unsafe extern "C" fn(P, c_int) -> c_int>(b"glfwGetMouseButton\0")?;
        let focused = self
            .symbol::<unsafe extern "C" fn(P, c_int) -> c_int>(b"glfwGetWindowAttrib\0")?(
            self.handle,
            0x00020001,
        ) != 0;
        let capture = focused
            && self
                .pointer_mode
                .map_or(mouse(self.handle, 1) == 1, |pointer| !pointer);
        let mut x = 0.;
        let mut y = 0.;
        self.symbol::<unsafe extern "C" fn(P, *mut f64, *mut f64)>(b"glfwGetCursorPos\0")?(
            self.handle,
            &mut x,
            &mut y,
        );
        // GLFW's disabled cursor keeps an unbounded virtual position, including
        // beyond monitor edges. This is general pointer motion, independent of a handle press.
        let (width,height)=self.logical_dimensions()?;
        let held=focused && mouse(self.handle,0)==1;
        let pointer_ui=self.pointer_mode==Some(true);
        let at_edge=x<=2. || y<=2. || x>=width-2. || y>=height-2.;
        let near_edge=x<=6. || y<=6. || x>=width-6. || y>=height-6.;
        let virtual_next=pointer_ui && focused && if self.virtual_drag {near_edge}else{at_edge};
        if virtual_next!=self.virtual_drag {
            self.symbol::<unsafe extern "C" fn(P,c_int,c_int)>(b"glfwSetInputMode\0")?(
                self.handle,0x00033001,if virtual_next {0x00034003}else{0x00034001});
            if !virtual_next {
                // Deliver this unbounded position below, then park the OS
                // cursor on the visible boundary for the next interaction.
                self.symbol::<unsafe extern "C" fn(P,f64,f64)>(b"glfwSetCursorPos\0")?(
                    self.handle,x.clamp(0.,(width-1.).max(0.)),y.clamp(0.,(height-1.).max(0.)));
            }
            self.virtual_drag=virtual_next;
        }
        #[cfg(target_os="macos")]
        {
            unsafe extern "C" {fn ww_workstation_virtual_cursor(window:P,x:f64,y:f64,dx:f64,dy:f64,visible:bool);}
            let edge_x=x.clamp(0.,width);let edge_y=y.clamp(0.,height);
            let outside=x!=edge_x || y!=edge_y;
            let cocoa=self.symbol::<unsafe extern "C" fn(P)->P>(b"glfwGetCocoaWindow\0")?;
            ww_workstation_virtual_cursor(cocoa(self.handle),edge_x,edge_y,x-edge_x,y-edge_y,self.virtual_drag&&outside);
        }
        let pointer_look=if focused && self.pointer_mode==Some(true){[(x-self.cursor[0]).clamp(-1000.,1000.),(y-self.cursor[1]).clamp(-1000.,1000.)]}else{[0.,0.]};
        let look = if capture && self.captured {
            [
                (x - self.cursor[0]).clamp(-1000., 1000.),
                (y - self.cursor[1]).clamp(-1000., 1000.),
            ]
        } else {
            [0., 0.]
        };
        if capture != self.captured && !self.virtual_drag {
            self.symbol::<unsafe extern "C" fn(P, c_int, c_int)>(b"glfwSetInputMode\0")?(
                self.handle,
                0x00033001,
                if capture { 0x00034003 } else { 0x00034001 },
            );
        }
        self.captured = capture;
        self.cursor = [x, y];
        let mut movement = [87, 83, 65, 68, 32, 340].map(|k| focused && key(self.handle, k) == 1);
        movement[5] |= focused && key(self.handle, 344) == 1;
        let select = focused && mouse(self.handle, 0) == 1;
        // Option/Alt + left drag exposes the same independent reach/twist axes
        // as the two-contact touch gesture; ordinary drag stays height/lateral.
        let depth_drag = select && (key(self.handle, 342) == 1 || key(self.handle, 346) == 1);
        let tool_delta = if depth_drag { [0.; 2] } else { look };
        let tool_twist = if depth_drag { look[0] } else { 0. };
        let tool_reach = if depth_drag { -look[1] } else { 0. };
        let quit = native_actions & 16 != 0 || self.workstation_keys.borrow_mut().safety.take_quit() || focused
            && cfg!(target_os = "macos")
            && key(self.handle, 81) == 1
            && (key(self.handle, 343) == 1 || key(self.handle, 347) == 1);
        let water_edit = if focused && key(self.handle, 70) == 1 {
            if key(self.handle, 340) == 1 || key(self.handle, 344) == 1 {
                serde_json::json!({"operation":"source","start":null})
            } else {
                serde_json::json!({"operation":"source"})
            }
        } else if focused && key(self.handle, 89) == 1 {
            serde_json::json!({"operation":"place","new_chain":key(self.handle,340)==1})
        } else if focused && key(self.handle, 91) == 1 {
            serde_json::json!({"operation":"radius","steps":-1})
        } else if focused && key(self.handle, 93) == 1 {
            serde_json::json!({"operation":"radius","steps":1})
        } else if focused && key(self.handle, 85) == 1 {
            serde_json::json!({"operation":"material"})
        } else if focused && key(self.handle, 259) == 1 {
            serde_json::json!({"operation":"clear","all":key(self.handle,340)==1})
        } else {
            serde_json::Value::Null
        };
        #[cfg(target_os="macos")]
        let pause_menu=if self.pointer_mode==Some(true) {self.pause_menu_input()?}else{serde_json::Value::Null};
        #[cfg(not(target_os="macos"))]
        let pause_menu=serde_json::Value::Null;
        let panel_keys={let mut keys=self.workstation_keys.borrow_mut();if !focused {keys.clear();}
            for (i,code) in [265,264,263,262].into_iter().enumerate() {
                if native_actions & (1<<i)!=0 {keys.safety.record(code,1,0,cfg!(target_os="macos"));}
            }
            keys.safety.panels([265,264,263,262].map(|k|focused&&key(self.handle,k)==1))};
        let mut input =
            serde_json::json!({"workstation":{"panels":panel_keys,"all":focused&&key(self.handle,32)==1,"expand":focused&&(key(self.handle,342)==1||key(self.handle,346)==1),"overview":focused&&workstation_overview(key(self.handle,340)==1||key(self.handle,344)==1,key(self.handle,341)==1||key(self.handle,345)==1||key(self.handle,343)==1||key(self.handle,347)==1)},"tools":{"slot":(0..4).find(|i|focused&&key(self.handle,49+*i)==1),"work":select,"pour":focused&&mouse(self.handle,1)==1},"cheat_keys":[focused&&key(self.handle,297)==1,focused&&key(self.handle,298)==1,focused&&key(self.handle,299)==1],"water_edit":water_edit,"sit":focused&&key(self.handle,90)==1,"park_gate":focused&&key(self.handle,79)==1,"move":movement,"look":look,"pointer_look":pointer_look,"reset":focused&&key(self.handle,82)==1,"pause":focused&&key(self.handle,80)==1,"step":focused&&key(self.handle,78)==1,"dig":focused&&(key(self.handle,69)==1||key(self.handle,71)==1||(self.pointer_mode==Some(false)&&select)),"tool_input":{"held":select,"rotate":focused&&mouse(self.handle,1)==1,"drop":focused&&key(self.handle,71)==1,"delta":tool_delta,"twist":tool_twist,"reach":tool_reach},"select":select,"focused":focused,"quit":quit,"boost":focused&&key(self.handle,341)==1});
        input["pause_menu"]=pause_menu;
        input["workstation"]["physical_key_presses"]=serde_json::json!(self.workstation_keys.borrow().relevant_presses);
        input["workstation"]["native_panel_actions"]=serde_json::json!(native_actions);
        input["workstation"]["native_focus"]=serde_json::json!({"app_active":native_focus&1!=0,"key_window":native_focus&2!=0,"content_first_responder":native_focus&4!=0,"regular_application":native_focus&8!=0,"key_eligible":native_focus&16!=0,"visible":native_focus&32!=0});
        // Absolute logical coordinates are independent of captured game look.
        // The workstation and focused phone interpret these pointer fields.
        let held=|k| focused && key(self.handle,k)==1;
        input["npc_talk"]=serde_json::json!(held(84));
        input["glasses"]=serde_json::json!({"interact":held(70),"equip":held(66),"open":held(69)});
        input["phone"]=serde_json::json!({"interact":held(70),"focus":held(82)});
        let shift=held(340)||held(344);
        let command=held(341)||held(345)||held(343)||held(347);
        let wheel=self.workstation_scroll.replace([0.;2]);
        let browser_keys=std::mem::take(&mut self.workstation_keys.borrow_mut().browser_keys);
        let (export_pressed,piano_press)=self.workstation_keys.borrow_mut().take(focused);
        let loop_range_pressed=std::mem::take(&mut self.workstation_keys.borrow_mut().loop_range);
        let preamp_switch_pressed=std::mem::take(&mut self.workstation_keys.borrow_mut().preamp_switch);
        let freeze_track_pressed=std::mem::take(&mut self.workstation_keys.borrow_mut().freeze_track);
        let split_clip_pressed=std::mem::take(&mut self.workstation_keys.borrow_mut().split_clip);
        let create_range_pressed=std::mem::take(&mut self.workstation_keys.borrow_mut().range_create);
        let transport_toggle_pressed=std::mem::take(&mut self.workstation_keys.borrow_mut().transport_toggle);
        let transport_toggle_timeline_pressed=std::mem::take(&mut self.workstation_keys.borrow_mut().transport_toggle_timeline);
        let (transport_events,transport_pointer_cancelled)=self.workstation_keys.borrow_mut().take_mouse();
        let transport_events:Vec<_>=transport_events.into_iter().map(|e|serde_json::json!({"pointer":e.point,"left":e.down})).collect();
        let midi_cancelled=std::mem::take(&mut self.workstation_keys.borrow_mut().midi_cancelled);
        input["workstation"]["editor"]=serde_json::json!({
            "security_camera":{"cycle":held(67),"next_camera":held(86),"pan":[held(65),held(68),held(87),held(83)]},
            "sfx_key_press":std::mem::take(&mut self.workstation_keys.borrow_mut().sfx_key_press),"focused":focused,"computer_midi":{"keys":crate::workstation::midi_keyboard::KEY_MAP.map(|(key,_)|held(key)),"octave":[held(90),held(88)],"cancelled":midi_cancelled},"transport_toggle_pressed":transport_toggle_pressed,"transport_toggle_timeline_pressed":transport_toggle_timeline_pressed,"transport_pointer_events":transport_events,"transport_pointer_cancelled":transport_pointer_cancelled,
            "pointer":if focused {serde_json::json!([x,y])}else{serde_json::Value::Null},
            "left":select,"right":focused&&mouse(self.handle,1)==1,
            "wheel":if focused {wheel}else{[0.;2]},
            "shift":shift,"alt":held(342)||held(346),"command":command,
            "preamp_switch_pressed":preamp_switch_pressed,"preamp_switch":held(90)&&!command&&!shift&&!held(342)&&!held(346),"delete":held(259)||held(261),"escape":self.sampled_escape.get(),"draw":held(80),"paint":held(66),
            "snap_toggle":held(71),"tab":held(258),"enter":held(257),"home":held(268),
            "browser_keys":browser_keys,"freeze_track_pressed":freeze_track_pressed,"key_events":true,"split_clip_pressed":split_clip_pressed,"create_range_pressed":create_range_pressed,"loop_range_pressed":loop_range_pressed,
            "enter_pressed":piano_press.is_some_and(|p|p.key==257),
            "enter_modifiers":piano_press.filter(|p|p.key==257).map_or(0,|p|p.mods),
            "undo":command&&held(90)&&!shift,"redo":command&&held(90)&&shift,
            "export":export_pressed,
            "arrows":[held(263),held(262),held(265),held(264)]
        });
        Ok(input)
    }
    pub unsafe fn keys(&self) -> Result<[bool; 3], String> {
        let get = self.symbol::<unsafe extern "C" fn(P, c_int) -> c_int>(b"glfwGetKey\0")?;
        let (escape,pressed)=self.workstation_keys.borrow_mut().safety.escape(get(self.handle,256)==1);
        self.sampled_escape.set(escape);self.escape_press.set(pressed);
        Ok([
            get(self.handle, 32) == 1,
            escape,
            get(self.handle, 81) == 1,
        ])
    }
    pub(super) fn escape_pressed(&self)->bool {self.escape_press.get()}
    pub unsafe fn gl_proc(&self, name: &CStr) -> Result<*const c_void, String> {
        let get = self.symbol::<unsafe extern "C" fn(*const c_char) -> *const c_void>(
            b"glfwGetProcAddress\0",
        )?;
        let address = get(name.as_ptr());
        if address.is_null() {
            Err(format!(
                "missing OpenGL function {}",
                name.to_string_lossy()
            ))
        } else {
            Ok(address)
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        REFRESH_FLAGS.with(|flags| {
            flags.borrow_mut().remove(&(self.handle as usize));
        });
        WORKSTATION_SCROLL.with(|map| {map.borrow_mut().remove(&(self.handle as usize));});
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().remove(&(self.handle as usize));});
        unsafe {
            // Balance capture before destroying its owning Cocoa/GLFW window,
            // including error paths that never reach graphics suspension.
            let _ = self.input_mode(true);
            (self.destroy)(self.handle);
        }
    }
}

#[cfg(test)]
mod workstation_key_tests {
    use super::*;
    #[test]
    fn workstation_shortcut_retains_a_complete_press_release_between_frames() {
        let handle=1usize as P;
        let flags=Rc::new(RefCell::new(WorkstationKeys::default()));
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().insert(1,Rc::downgrade(&flags));});
        for modifiers in [0x0001|0x0002,0x0001|0x0008] {
            unsafe {
                workstation_key(handle,69,0,1,modifiers);
                workstation_key(handle,69,0,0,0);
            }
            assert_eq!(flags.borrow_mut().take(true),(true,None));
            assert_eq!(flags.borrow_mut().take(true),(false,None));
        }
        unsafe {
            workstation_key(handle,69,0,1,0x0002);
            workstation_key(handle,69,0,1,0x0001);
            workstation_key(handle,69,0,2,0x0001|0x0008);
        }
        assert_eq!(flags.borrow_mut().take(true),(false,None));
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().remove(&1);});
        // A late event after window release cannot target retained or new state.
        unsafe {workstation_key(handle,69,0,1,0x0001|0x0008);}
        assert_eq!(flags.borrow_mut().take(true),(false,None));
    }
    #[test]
    fn workstation_piano_presses_preserve_order_modifiers_and_repeated_taps() {
        let handle=2usize as P;
        let flags=Rc::new(RefCell::new(WorkstationKeys::default()));
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().insert(2,Rc::downgrade(&flags));});
        for (key,mods) in [(257,0x0001),(257,0)] {
            unsafe {
                workstation_key(handle,key,0,1,mods);
                workstation_key(handle,key,0,2,mods); // Repeat is not a fresh press.
                workstation_key(handle,key,0,0,0);
            }
        }
        for (key,mods) in [(257,0x0001),(257,0)] {
            assert_eq!(flags.borrow_mut().take(true),(false,Some(PianoPress{key,mods})));
        }
        assert_eq!(flags.borrow_mut().take(true),(false,None));
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().remove(&2);});
    }
    #[test]
    fn workstation_focus_loss_and_unfocused_drain_discard_bounded_pending_commands() {
        let handle=3usize as P;
        let flags=Rc::new(RefCell::new(WorkstationKeys::default()));
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().insert(3,Rc::downgrade(&flags));});
        unsafe {
            workstation_key(handle,69,0,1,0x0001|0x0008);
            workstation_key(handle,78,0,1,0);
            workstation_focus(handle,0);
            workstation_focus(handle,1); // Losing/regaining focus within one poll cannot replay.
        }
        assert_eq!(flags.borrow_mut().take(true),(false,None));
        for _ in 0..32 {unsafe {workstation_key(handle,257,0,1,0);}}
        assert_eq!(flags.borrow().piano.len(),8);
        assert_eq!(flags.borrow_mut().take(false),(false,None));
        assert_eq!(flags.borrow_mut().take(true),(false,None));
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().remove(&3);});
    }
    #[test]
    fn buffered_panel_escape_and_quit_use_the_real_callback_path() {
        let handle=4usize as P;
        let flags=Rc::new(RefCell::new(WorkstationKeys::default()));
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().insert(4,Rc::downgrade(&flags));});
        unsafe {
            for key in [265,256] {workstation_key(handle,key,0,1,0);workstation_key(handle,key,0,0,0);}
        }
        assert_eq!(flags.borrow_mut().safety.panels([false;4]),[true,false,false,false]);
        assert_eq!(flags.borrow_mut().safety.escape(false),(true,true));
        assert_eq!(flags.borrow().relevant_presses,[1,0,0,0,1,0]);
        assert_eq!(flags.borrow_mut().take(true),(false,None)); // Existing consumers do not erase safety events.
        unsafe {workstation_key(handle,81,0,1,8);workstation_key(handle,81,0,0,0);workstation_focus(handle,0);}
        assert_eq!(flags.borrow_mut().take(false),(false,None));
        assert_eq!(flags.borrow_mut().safety.take_quit(),cfg!(target_os="macos"));
        assert!(!flags.borrow_mut().safety.take_quit());
        WORKSTATION_KEYS.with(|map| {map.borrow_mut().remove(&4);});
    }
}

#[cfg(test)] mod cardinal_range_keys_tests {
 use super::*;
 #[test]fn chord_buffers_short_press_once_and_ignores_repeat_or_wrong_modifiers(){
  let cmd=if cfg!(target_os="macos"){8}else{2};let mut k=WorkstationKeys::default();
  for mods in [0,1,cmd] {k.record(77,1,mods);k.record(77,0,mods);assert!(!k.range_create);}
  k.record(77,1,cmd|1);k.record(77,2,cmd|1);k.record(77,0,0);
  assert!(std::mem::take(&mut k.range_create));assert!(!k.range_create);
  k.record(78,1,0);assert!(k.piano.is_empty());
  if cfg!(target_os="macos"){k.record(77,1,3);assert!(!k.range_create);}
 }
 #[test]fn modifiers_last_focus_loss_and_overview_are_safe(){
  let cmd=if cfg!(target_os="macos"){8}else{2};let mut k=WorkstationKeys::default();
  k.record(77,1,0);k.record(340,1,1);k.record(if cfg!(target_os="macos"){343}else{341},1,cmd|1);
  assert!(std::mem::take(&mut k.range_create));k.record(77,2,cmd|1);assert!(!k.range_create);
  k.clear();assert!(!k.m_down&&!k.range_chord_down&&!k.range_create);
  assert!(workstation_overview(true,false));assert!(!workstation_overview(true,true));
 }
}

#[cfg(test)]mod transport_mouse_tests {
 use super::*;
 thread_local!{static CURSOR:Cell<[f64;2]>=Cell::new([0.;2]);}
 unsafe extern "C" fn cursor(_handle:P,x:*mut f64,y:*mut f64){let p=CURSOR.with(Cell::get);*x=p[0];*y=p[1];}
 #[test]fn native_callback_keeps_press_release_points_between_frame_samples(){
  let handle=51usize as P;let flags=Rc::new(RefCell::new(WorkstationKeys::default()));flags.borrow_mut().cursor_position=Some(cursor);
  WORKSTATION_KEYS.with(|m|{m.borrow_mut().insert(51,Rc::downgrade(&flags));});
  CURSOR.with(|c|c.set([100.,50.]));unsafe{workstation_mouse(handle,0,1,0);}
  CURSOR.with(|c|c.set([120.,60.]));unsafe{workstation_mouse(handle,0,0,0);}
  let(e,cancel)=flags.borrow_mut().take_mouse();assert!(!cancel);assert_eq!(e.len(),2);assert_eq!(e[0].point,[100.,50.]);assert!(e[0].down);assert_eq!(e[1].point,[120.,60.]);assert!(!e[1].down);
  assert!(flags.borrow_mut().take_mouse().0.is_empty());WORKSTATION_KEYS.with(|m|{m.borrow_mut().remove(&51);});
 }
 #[test]fn focus_and_overflow_cancel_without_replaying_partial_gestures(){
  let mut k=WorkstationKeys::default();k.record_mouse([1.,2.],true);k.clear();let(e,c)=k.take_mouse();assert!(e.is_empty()&&c);
  for i in 0..17{k.record_mouse([1.,2.],i%2==0);}let(e,c)=k.take_mouse();assert!(e.is_empty()&&c);
  k.record_mouse([1.,2.],false);assert_eq!(k.take_mouse().0.len(),1);
 }
}

#[cfg(test)]mod transport_space_key_tests {
 use super::*;
 #[test]fn callback_preserves_timeline_modifiers_and_piano_roll_space(){
  let handle=52usize as P;let flags=Rc::new(RefCell::new(WorkstationKeys::default()));
  WORKSTATION_KEYS.with(|m|{m.borrow_mut().insert(52,Rc::downgrade(&flags));});
  let command=if cfg!(target_os="windows"){2}else{8};
  for (mods,allowed) in [(0,true),(1,true),(command,false),(4,false),(command|4,false)] {
   unsafe{workstation_key(handle,32,0,1,mods);workstation_key(handle,32,0,0,0);}
   let mut k=flags.borrow_mut();assert!(std::mem::take(&mut k.transport_toggle));
   assert_eq!(std::mem::take(&mut k.transport_toggle_timeline),allowed);
  }
  unsafe{workstation_key(handle,32,0,2,0);}assert!(!flags.borrow().transport_toggle);
  unsafe{workstation_key(handle,32,0,1,0);workstation_focus(handle,0);}
  assert!(!flags.borrow().transport_toggle&&!flags.borrow().transport_toggle_timeline);
  WORKSTATION_KEYS.with(|m|{m.borrow_mut().remove(&52);});
 }
}

#[cfg(test)]mod timeline_loop_key_tests {
 use super::*;
 #[test]fn loop_selection_chord_keeps_short_presses_and_rejects_shift_repeat(){
  let cmd=if cfg!(target_os="windows"){2}else{8};let mut k=WorkstationKeys::default();
  for mods in [0,1,cmd|1] {k.record(76,1,mods);k.record(76,0,0);assert!(!k.loop_range);}
  k.record(76,1,cmd);k.record(76,2,cmd);k.record(76,0,0);
  assert!(std::mem::take(&mut k.loop_range));k.record(76,2,cmd);assert!(!k.loop_range);
  k.clear();k.record(76,1,0);k.record(if cfg!(target_os="windows"){341}else{343},1,cmd);
  assert!(std::mem::take(&mut k.loop_range));k.record(32,1,0);k.clear();
  assert!(!k.loop_range&&!k.loop_chord_down&&!k.l_down&&!k.transport_toggle&&!k.transport_toggle_timeline);
 }
}

#[cfg(test)]mod clip_split_shortcut_tests {
 use super::*;
 #[test]fn command_e_buffers_short_tap_ignores_repeat_and_clears_on_blur(){let mut keys=WorkstationKeys::default();let cmd=if cfg!(target_os="macos"){8}else{2};keys.record(69,1,0);assert!(!keys.split_clip);keys.record(69,0,0);keys.record(69,1,cmd);keys.record(69,0,0);assert!(std::mem::take(&mut keys.split_clip));keys.record(69,2,cmd);assert!(!keys.split_clip);keys.record(69,1,cmd);keys.clear();assert!(!keys.split_clip);}
}
#[cfg(test)]mod track_freeze_key_tests{
 use super::WorkstationKeys;
 #[test]fn freeze_key_buffers_short_taps_ignores_repeats_modifiers_and_blur(){let mut k=WorkstationKeys::default();k.record(70,1,0);k.record(70,0,0);assert!(std::mem::take(&mut k.freeze_track));k.record(70,2,0);assert!(!k.freeze_track);for mods in [1,2,4,8]{k.record(70,1,mods);assert!(!k.freeze_track);}k.record(70,1,0);k.clear();assert!(!k.freeze_track);}
}
