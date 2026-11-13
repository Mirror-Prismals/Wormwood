//! OpenGL ES 3 connection for bounded scene batches and the original Pelagic mesh.
use serde_json::{json, Value};
use std::{
    ffi::{c_void, CString},
    path::Path,
};
unsafe extern "C" {
    fn ww_scene_open(
        vertex: *const i8,
        fragment: *const i8,
        error: *mut u8,
        capacity: usize,
        workstation: bool,
    ) -> *mut c_void;
    fn ww_scene_table_eye(handle:*mut c_void,eye:*const f32,light:*const f32);
    fn ww_scene_view(handle: *mut c_void) -> *mut c_void;
    fn ww_scene_cloud_frame(handle: *mut c_void, frame: *const f32);
    fn ww_scene_cloud_noise(handle: *mut c_void, width: u32, height: u32, pixels: *const u8);
    fn ww_scene_woodland(handle: *mut c_void, width: u32, height: u32, pixels: *const u8);
    fn ww_scene_workstation(handle: *mut c_void, rows: *const f32, count: u32);
    fn ww_scene_texture(handle: *mut c_void, width: u32, height: u32, pixels: *const u8);
    fn ww_scene_batch(handle: *mut c_void, vertices: *const f32, count: u32);
    fn ww_scene_opaque(handle: *mut c_void);
    fn ww_scene_with_context(
        handle: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, u32, u32, u32) -> i32,
        context: *mut c_void,
    ) -> i32;
    fn ww_scene_draw(
        handle: *mut c_void,
        clear: *const f32,
        begin: Option<unsafe extern "C" fn(*mut c_void, u32, u32, u32) -> i32>,
        overlay: Option<unsafe extern "C" fn(*mut c_void, u32, u32, u32) -> i32>,
        context: *mut c_void,
        validate: bool,
        center: *mut u8,
        corner: *mut u8,
        error: *mut u8,
        capacity: usize,
    ) -> i32;
    fn ww_scene_model(
        handle: *mut c_void,
        vertices: *const f32,
        bones: u32,
        shell: u32,
        vertex: *const i8,
        fragment: *const i8,
        error: *mut u8,
        capacity: usize,
    ) -> i32;
    fn ww_scene_model_frame(handle: *mut c_void, frame: *const f32);
    fn ww_graphics_close(handle: *mut c_void, error: *mut u8, capacity: usize) -> i32;
}
pub(super) struct Client {
    pub window: super::window::Window,
    handle: *mut c_void,
    water: Option<super::water::opengl::Client>,
    water_targets: Option<super::water::opengl::SceneTargets>,
    water_frames: Vec<super::water::data::DrawFrame>,
    water_packets: Vec<crate::water::presentation::Packet>,
    scene_view: Value,
    scene_triangles: Vec<[f32; 24]>,
    reflection_vegetation: Option<super::vegetation::opengl::Client>,
    vegetation: Option<super::vegetation::opengl::Client>,
    plant_packet: Option<crate::vegetation::packet::Packet>,
    root: std::path::PathBuf,
    overlay_error: Option<String>,
    pub info: Value,
    pub clear: [f32; 4],
    pub captured: bool,
    pub validate_triangle_center: bool,
}
fn message(error: &[u8]) -> String {
    String::from_utf8_lossy(error.split(|b| *b == 0).next().unwrap_or(error)).into_owned()
}
impl Client {
    pub unsafe fn new(
        _root: &Path,
        data: &[f32; 24],
        _: &Path,
        workstation: bool,
    ) -> Result<Self, String> {
        let vs = CString::new(
            std::fs::read_to_string(_root.join(if workstation {
                "Procedures/Shaders/workstation.ios.vert"
            } else {
                "Procedures/Shaders/scene.ios.vert"
            }))
            .map_err(|e| e.to_string())?,
        )
        .unwrap();
        let fs = CString::new(
            crate::workstation::style_shader::source(_root, if workstation {
                "Procedures/Shaders/workstation.ios.frag"
            } else {
                "Procedures/Shaders/scene.ios.frag"
            })?,
        )
        .unwrap();
        let mut error = [0; 1024];
        let handle = unsafe {
            ww_scene_open(
                vs.as_ptr(),
                fs.as_ptr(),
                error.as_mut_ptr(),
                error.len(),
                workstation,
            )
        };
        if handle.is_null() {
            return Err(message(&error));
        }
        let window = super::window::Window {
            handle: unsafe { ww_scene_view(handle) },
        };
        let mut s = Self {
            window,
            handle,
            water: None,
            water_targets: None,
            water_frames: Vec::new(),
            water_packets: Vec::new(),
            scene_view: Value::Null,
            scene_triangles: Vec::new(),
            reflection_vegetation: None,
            vegetation: None,
            plant_packet: None,
            root: _root.into(),
            overlay_error: None,
            info: json!({"api":"OpenGL ES 3","readback_verified":false}),
            clear: [0.035, 0.045, 0.065, 1.],
            captured: false,
            validate_triangle_center: true,
        };
        unsafe {
            let noise = crate::scene_assets::gl_atlas();
            ww_scene_cloud_noise(s.handle, 528, (noise.len() / (528 * 4)) as u32, noise.as_ptr());
            let atlas = crate::terrain::woodland::BYTES;
            ww_scene_woodland(
                s.handle,
                24,
                (24 * crate::terrain::woodland::TILE_COUNT) as u32,
                atlas.as_ptr().add(8),
            );
            s.texture(&[1, 0, 0, 0, 1, 0, 0, 0, 255, 255, 255, 255])?;
        }
        unsafe { s.set_batch(std::slice::from_ref(data))? };
        if workstation {
            s.set_workstation(&super::workstation::initial(_root)?)?;
        }
        Ok(s)
    }
    pub unsafe fn set_water(
        &mut self,
        _root: &Path,
        packets: &[crate::water::presentation::Packet],
        view: &Value,
        triangles: &[[f32; 24]],
    ) -> Result<(), String> {
        let cloud_frame = crate::base_system::layer_four::cloud_system::read_frame(view)?;
        ww_scene_cloud_frame(self.handle, cloud_frame.as_ptr().cast());

        self.water_packets = packets.to_vec();
        self.scene_view = view.clone();
        self.scene_triangles = triangles.to_vec();
        Ok(())
    }
    unsafe extern "C" fn begin_scene(
        context: *mut c_void,
        _target: u32,
        width: u32,
        height: u32,
    ) -> i32 {
        let client = unsafe { &mut *context.cast::<Self>() };
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                unsafe {
                    client.water_frames.clear();
                    if client.water_packets.is_empty() {
                        return Ok(());
                    }
                    client.water_frames = super::scene::water_frames(
                        &client.water_packets,
                        &client.scene_view,
                        width,
                        height,
                        true,
                    )?;
                    if client.water.is_none() {
                        let library = libloading::os::unix::Library::this();
                        let mut resolve = |name: &std::ffi::CStr| {
                            library
                                .get::<*const c_void>(name.to_bytes_with_nul())
                                .map(|s| *s)
                                .unwrap_or(std::ptr::null())
                        };
                        client.water = Some(super::water::opengl::Client::new(
                            &client.root,
                            &mut resolve,
                        )?);
                        client.water_targets =
                            Some(super::water::opengl::SceneTargets::new(&mut resolve)?);
                    }
                    let targets = client.water_targets.as_mut().unwrap();
                    targets.resize(width, height)?;
                    if let Some(frame) = client
                        .water_frames
                        .iter()
                        .find(|f| f.optics.planar_reflection)
                    {
                        let reflected =
                            super::scene::reflected_batch(&client.scene_triangles, frame)?;
                        let pack = |triangles: &[[f32; 24]]| -> Vec<f32> {
                            let mut data = Vec::with_capacity(triangles.len() * 24);
                            for t in triangles {
                                for i in 0..3 {
                                    data.extend_from_slice(&t[i * 4..i * 4 + 4]);
                                    data.extend_from_slice(&t[12 + i * 4..16 + i * 4]);
                                }
                            }
                            data
                        };
                        targets.begin(true, client.clear);
                        let reflected = pack(&reflected);
                        ww_scene_batch(
                            client.handle,
                            reflected.as_ptr(),
                            (reflected.len() / 8) as u32,
                        );
                        ww_scene_opaque(client.handle);
                        if let Some(packet) = &client.plant_packet {
                            let reflected = super::scene::reflected_plants(packet, frame)?;
                            if client.reflection_vegetation.is_none() {
                                let library = libloading::os::unix::Library::this();
                                client.reflection_vegetation =
                                    Some(super::vegetation::opengl::Client::new(
                                        &client.root,
                                        &mut |name| {
                                            library
                                                .get::<*const c_void>(name.to_bytes_with_nul())
                                                .map(|s| *s)
                                                .unwrap_or(std::ptr::null())
                                        },
                                    )?);
                            }
                            client.reflection_vegetation.as_mut().unwrap().draw(
                                &reflected,
                                targets.reflection_framebuffer(),
                                width,
                                height,
                            )?;
                        }
                        let original = pack(&client.scene_triangles);
                        ww_scene_batch(
                            client.handle,
                            original.as_ptr(),
                            (original.len() / 8) as u32,
                        );
                    }
                    targets.begin(false, client.clear);
                    Ok(())
                }
            }));
        match result {
            Ok(Ok(())) => 0,
            Ok(Err(error)) => {
                client.overlay_error = Some(error);
                -1
            }
            Err(_) => {
                client.overlay_error = Some("water scene setup panicked".into());
                -1
            }
        }
    }
    pub unsafe fn set_vegetation(
        &mut self,
        _root: &Path,
        packet: Option<crate::vegetation::packet::Packet>,
    ) -> Result<(), String> {
        self.plant_packet = packet;
        Ok(())
    }
    unsafe extern "C" fn overlay(
        context: *mut c_void,
        target: u32,
        width: u32,
        height: u32,
    ) -> i32 {
        let client = unsafe { &mut *context.cast::<Self>() };
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                if let Some(packet) = &client.plant_packet {
                    if client.vegetation.is_none() {
                        let library = libloading::os::unix::Library::this();
                        client.vegetation = Some(unsafe {
                            super::vegetation::opengl::Client::new(&client.root, &mut |name| {
                                library
                                    .get::<*const c_void>(name.to_bytes_with_nul())
                                    .map(|s| *s)
                                    .unwrap_or(std::ptr::null())
                            })?
                        });
                    }
                    unsafe {
                        client.vegetation.as_mut().unwrap().draw(
                            packet,
                            client
                                .water_targets
                                .as_ref()
                                .filter(|_| !client.water_frames.is_empty())
                                .map_or(target, |t| t.framebuffer()),
                            width,
                            height,
                        )?;
                    }
                }
                if !client.water_frames.is_empty() {
                    unsafe {
                        client.water.as_mut().unwrap().encode_batch(
                            &client.water_frames,
                            client.water_targets.as_ref().unwrap().attachments(
                                target,
                                client
                                    .water_frames
                                    .iter()
                                    .any(|f| f.optics.planar_reflection),
                            ),
                        )?;
                    }
                }
                Ok(())
            }));
        match result {
            Ok(Ok(())) => 0,
            Ok(Err(error)) => {
                client.overlay_error = Some(error);
                -1
            }
            Err(_) => {
                client.overlay_error = Some("vegetation rendering panicked".into());
                -1
            }
        }
    }
    unsafe extern "C" fn release_overlay(
        context: *mut c_void,
        _target: u32,
        _width: u32,
        _height: u32,
    ) -> i32 {
        let client = unsafe { &mut *context.cast::<Self>() };
        client.vegetation.take();
        client.reflection_vegetation.take();
        client.water.take();
        client.water_targets.take();
        0
    }
    pub unsafe fn set_world_backdrop(&mut self,_root:&Path,enabled:bool)->Result<bool,String>{
        if enabled{return Err("full world workstation composition is not integrated with the iOS OpenGL ES surface".into());}Ok(false)
    }
    pub unsafe fn set_workstation(&mut self, rows: &[[f32; 4]]) -> Result<(), String> {
        if rows.is_empty() || rows.len() % 256 != 0 {
            return Err("unaligned workstation optical texture".into());
        }
        ww_scene_workstation(self.handle, rows.as_ptr().cast(), rows.len() as u32);
        Ok(())
    }
    pub unsafe fn table_eye(&mut self,view:&Value)->Result<(),String>{
        let eye=crate::scene_assets::table_eye(view)?;let light=crate::scene_assets::exterior_light(view);
        unsafe{ww_scene_table_eye(self.handle,eye.as_ptr(),light.as_ptr());}Ok(())
    }
    pub unsafe fn monitor_image(&mut self,image:&[u8])->Result<(),String>{
        if image.len()!=crate::scene_assets::SCREEN_BYTES{return Err("invalid monitor image size".into());}
        let mut atlas=crate::scene_assets::CLOUD_NOISE[8..].to_vec();atlas.extend_from_slice(crate::scene_assets::FINISHES);
        atlas.extend_from_slice(image);atlas.extend_from_slice(&crate::scene_assets::TABLE[16..]);atlas.resize(atlas.len().div_ceil(528*4)*528*4,0);
        unsafe{ww_scene_cloud_noise(self.handle,528,(atlas.len()/(528*4)) as u32,atlas.as_ptr());}Ok(())
    }
    pub unsafe fn texture(&mut self, bytes: &[u8]) -> Result<(), String> {
        let w = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let h = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        unsafe {
            ww_scene_texture(self.handle, w, h, bytes.as_ptr().add(8));
        }
        Ok(())
    }
    pub unsafe fn set_batch(&mut self, triangles: &[[f32; 24]]) -> Result<(), String> {
        if triangles.len() > crate::scene_limits::MAX_SCENE_TRIANGLES {
            return Err("iOS triangle budget exceeded".into());
        }
        let mut data = Vec::with_capacity(triangles.len() * 24);
        for t in triangles {
            for i in 0..3 {
                data.extend_from_slice(&t[i * 4..i * 4 + 4]);
                data.extend_from_slice(&t[12 + i * 4..16 + i * 4]);
            }
        }
        unsafe { ww_scene_batch(self.handle, data.as_ptr(), (triangles.len() * 3) as u32) };
        Ok(())
    }
    pub unsafe fn draw(&mut self) -> Result<(), String> {
        let (mut center, mut corner) = ([0; 4], [0; 4]);
        let mut error = [0; 1024];
        if unsafe {
            ww_scene_draw(
                self.handle,
                self.clear.as_ptr(),
                Some(Self::begin_scene),
                Some(Self::overlay),
                (self as *mut Self).cast(),
                !self.captured && self.validate_triangle_center,
                center.as_mut_ptr(),
                corner.as_mut_ptr(),
                error.as_mut_ptr(),
                error.len(),
            )
        } != 0
        {
            return Err(self.overlay_error.take().unwrap_or_else(|| message(&error)));
        }
        if !self.captured {
            self.info["center_rgba"] = json!(center);
            self.info["corner_rgba"] = json!(corner);
            self.info["readback_verified"] = json!(self.validate_triangle_center);
            self.captured = true;
        }
        Ok(())
    }
    pub unsafe fn wait(&self) -> Result<(), String> {
        Ok(())
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let mut error = [0; 1024];
        unsafe {
            ww_scene_with_context(
                self.handle,
                Self::release_overlay,
                (self as *mut Self).cast(),
            );
            ww_graphics_close(self.handle, error.as_mut_ptr(), error.len());
        }
    }
}
