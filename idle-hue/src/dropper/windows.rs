use std::sync::Arc;

use crate::State;
use haven::winit::WinitApp;
use haven::*;

#[derive(Clone, Copy, Debug, Default)]
struct ScreenBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct ScreenPoint {
    x: i32,
    y: i32,
}

#[derive(Clone)]
struct DropperFrame {
    bounds: ScreenBounds,
    cursor: ScreenPoint,
    rgb: [f32; 3],
    image_size: u32,
    image: Arc<Vec<u8>>,
}

pub(crate) struct WindowsDropper {
    bounds: ScreenBounds,
    sampler: Option<ScreenSampler>,
    frame: Option<DropperFrame>,
    active: bool,
}

impl WindowsDropper {
    pub(crate) fn new() -> Self {
        Self {
            bounds: screen_bounds(),
            sampler: ScreenSampler::new(17, 9),
            frame: None,
            active: false,
        }
    }

    pub(crate) fn start(&mut self) {
        self.active = true;
        self.frame = None;
    }

    fn cancel(&mut self) {
        self.active = false;
        self.frame = None;
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn frame(&self) -> Option<DropperFrame> {
        self.active.then(|| self.frame.clone()).flatten()
    }

    fn rgb(&self) -> Option<[f32; 3]> {
        self.frame.as_ref().map(|frame| frame.rgb)
    }

    fn update(&mut self) {
        if !self.active {
            return;
        }
        let Some(cursor) = cursor_position() else {
            return;
        };
        let Some(sampler) = self.sampler.as_mut() else {
            return;
        };
        self.frame = sampler.capture(self.bounds, cursor);
    }
}

fn screen_bounds() -> ScreenBounds {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    unsafe {
        ScreenBounds {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1) as u32,
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1) as u32,
        }
    }
}

pub(crate) fn add_pane(app: WinitApp<State>) -> WinitApp<State> {
    let bounds = screen_bounds();
    app.pane(
        PaneBuilder::new("dropper", view)
            .title("idle-hue dropper")
            .initial_bounds(bounds.x, bounds.y, bounds.width, bounds.height)
            .resizable(false)
            .transparent(true)
            .decorations(false)
            .window_level(WindowLevel::AlwaysOnTop)
            .initially_active(true)
            .skip_taskbar(true)
            .cursor_visible(false)
            .open_at_start(false)
            .on_frame(on_frame)
            .on_exit(on_exit),
    )
}

fn on_frame(state: &mut State, app: &mut PaneState) {
    if state.dropper.is_active() {
        state.dropper.update();
        app.redraw();
    }
}

fn on_exit(state: &mut State, _app: &mut PaneState) {
    state.dropper.cancel();
}

fn view<'a>(s: &'a State, app: &mut PaneState) -> View<'a, State> {
    let pick =
        gesture::click(id!())
            .button(MouseButton::Left)
            .run(|state: &mut State, app, event| {
                if matches!(event.state, ClickPhase::Completed) {
                    state.dropper.update();
                    if let Some(rgb) = state.dropper.rgb() {
                        state.set_srgb(rgb, app);
                        state.save_state(app);
                    }
                    state.dropper.cancel();
                    app.close();
                }
            });
    let cancel_click =
        gesture::click(id!())
            .button(MouseButton::Right)
            .run(|state: &mut State, app, event| {
                if matches!(event.state, ClickPhase::Completed) {
                    state.dropper.cancel();
                    app.close();
                }
            });
    let cancel_key =
        gesture::key(id!())
            .key(NamedKey::Escape)
            .run(|state: &mut State, app, event| {
                if event.phase == KeyPhase::Pressed {
                    state.dropper.cancel();
                    app.close();
                }
            });

    let mut layers = vec![
        rect(id!())
            .fill(Color::TRANSPARENT)
            .view()
            .gesture(pick)
            .gesture(cancel_click)
            .gesture(cancel_key)
            .build(app)
            .expand(),
    ];
    if let Some(frame) = s.dropper.frame() {
        layers.push(loupe(frame).expand());
    }
    stack(layers)
}

fn loupe(frame: DropperFrame) -> View<'static, State> {
    const LOUPE: f32 = 153.0;
    const GAP: f32 = 24.0;
    const CURSOR: f32 = 10.0;

    draw(move |area: Area, ctx: &mut PaneState| {
        if area.width <= 0.0 || area.height <= 0.0 {
            return Vec::new();
        }
        let scale_x = frame.bounds.width as f32 / area.width;
        let scale_y = frame.bounds.height as f32 / area.height;
        let cursor_x = (frame.cursor.x - frame.bounds.x) as f32 / scale_x;
        let cursor_y = (frame.cursor.y - frame.bounds.y) as f32 / scale_y;
        let mut loupe_x = cursor_x + GAP;
        let mut loupe_y = cursor_y + GAP;
        if loupe_x + LOUPE > area.width {
            loupe_x = cursor_x - LOUPE - GAP;
        }
        if loupe_y + LOUPE > area.height {
            loupe_y = cursor_y - LOUPE - GAP;
        }
        loupe_x = loupe_x.clamp(0.0, (area.width - LOUPE).max(0.0));
        loupe_y = loupe_y.clamp(0.0, (area.height - LOUPE).max(0.0));
        let loupe_area = Area {
            x: loupe_x,
            y: loupe_y,
            width: LOUPE,
            height: LOUPE,
        };
        let center_x = loupe_x + LOUPE * 0.5;
        let center_y = loupe_y + LOUPE * 0.5;
        let mut views = Vec::new();
        views.extend(
            image(
                80_001,
                ImageSource::Buffer(frame.image_size, frame.image_size, frame.image.clone()),
            )
            .corner_rounding(76.5)
            .finish(ctx)
            .draw(loupe_area, ctx),
        );
        views.extend(
            rect(80_002)
                .fill(Color::TRANSPARENT)
                .stroke(Color::WHITE.with_alpha(0.9), Stroke::new(2.0))
                .corner_rounding(76.5)
                .build(ctx)
                .draw(loupe_area, ctx),
        );
        views.extend(
            rect(80_003)
                .fill(Color::WHITE.with_alpha(0.85))
                .build(ctx)
                .draw(
                    Area {
                        x: center_x - LOUPE * 0.5,
                        y: center_y - 0.5,
                        width: LOUPE,
                        height: 1.0,
                    },
                    ctx,
                ),
        );
        views.extend(
            rect(80_004)
                .fill(Color::WHITE.with_alpha(0.85))
                .build(ctx)
                .draw(
                    Area {
                        x: center_x - 0.5,
                        y: center_y - LOUPE * 0.5,
                        width: 1.0,
                        height: LOUPE,
                    },
                    ctx,
                ),
        );
        views.extend(
            rect(80_005)
                .fill(Color::WHITE.with_alpha(0.95))
                .build(ctx)
                .draw(
                    Area {
                        x: cursor_x - CURSOR,
                        y: cursor_y,
                        width: CURSOR * 2.0,
                        height: 1.0,
                    },
                    ctx,
                ),
        );
        views.extend(
            rect(80_006)
                .fill(Color::WHITE.with_alpha(0.95))
                .build(ctx)
                .draw(
                    Area {
                        x: cursor_x,
                        y: cursor_y - CURSOR,
                        width: 1.0,
                        height: CURSOR * 2.0,
                    },
                    ctx,
                ),
        );
        views
    })
}

fn cursor_position() -> Option<ScreenPoint> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }
    Some(ScreenPoint {
        x: point.x,
        y: point.y,
    })
}

struct ScreenSampler {
    screen_dc: windows_sys::Win32::Graphics::Gdi::HDC,
    memory_dc: windows_sys::Win32::Graphics::Gdi::HDC,
    bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    previous: windows_sys::Win32::Graphics::Gdi::HGDIOBJ,
    bits: *mut u8,
    sample_size: u32,
    pixel_size: u32,
}

impl ScreenSampler {
    fn new(sample_size: u32, pixel_size: u32) -> Option<Self> {
        use std::ffi::c_void;
        use std::mem::size_of;
        use std::ptr::null_mut;
        use windows_sys::Win32::Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
            DIB_RGB_COLORS, GetDC, RGBQUAD, ReleaseDC, SelectObject,
        };

        unsafe {
            let screen_dc = GetDC(null_mut());
            if screen_dc.is_null() {
                return None;
            }
            let memory_dc = CreateCompatibleDC(screen_dc);
            if memory_dc.is_null() {
                let _ = ReleaseDC(null_mut(), screen_dc);
                return None;
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: sample_size as i32,
                    biHeight: -(sample_size as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: sample_size * sample_size * 4,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [RGBQUAD {
                    rgbBlue: 0,
                    rgbGreen: 0,
                    rgbRed: 0,
                    rgbReserved: 0,
                }],
            };
            let mut bits = null_mut::<c_void>();
            let bitmap =
                CreateDIBSection(screen_dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                let _ = windows_sys::Win32::Graphics::Gdi::DeleteDC(memory_dc);
                let _ = ReleaseDC(null_mut(), screen_dc);
                return None;
            }
            let previous = SelectObject(memory_dc, bitmap);
            Some(Self {
                screen_dc,
                memory_dc,
                bitmap,
                previous,
                bits: bits.cast(),
                sample_size,
                pixel_size,
            })
        }
    }

    fn capture(&mut self, bounds: ScreenBounds, cursor: ScreenPoint) -> Option<DropperFrame> {
        use windows_sys::Win32::Graphics::Gdi::{BitBlt, SRCCOPY};

        let half = (self.sample_size / 2) as i32;
        let max_x = bounds.x + bounds.width as i32 - self.sample_size as i32;
        let max_y = bounds.y + bounds.height as i32 - self.sample_size as i32;
        let x = (cursor.x - half).clamp(bounds.x, max_x.max(bounds.x));
        let y = (cursor.y - half).clamp(bounds.y, max_y.max(bounds.y));
        let copied = unsafe {
            BitBlt(
                self.memory_dc,
                0,
                0,
                self.sample_size as i32,
                self.sample_size as i32,
                self.screen_dc,
                x,
                y,
                SRCCOPY,
            )
        };
        if copied == 0 {
            return None;
        }
        let src = unsafe {
            std::slice::from_raw_parts(
                self.bits,
                (self.sample_size * self.sample_size * 4) as usize,
            )
        };
        let center = ((self.sample_size * self.sample_size / 2) * 4) as usize;
        let rgb = [
            src[center + 2] as f32 / 255.0,
            src[center + 1] as f32 / 255.0,
            src[center] as f32 / 255.0,
        ];
        let image_size = self.sample_size * self.pixel_size;
        let mut image = vec![0; (image_size * image_size * 4) as usize];
        for py in 0..self.sample_size {
            for px in 0..self.sample_size {
                let src_index = ((py * self.sample_size + px) * 4) as usize;
                let rgba = [src[src_index + 2], src[src_index + 1], src[src_index], 255];
                for y_scale in 0..self.pixel_size {
                    for x_scale in 0..self.pixel_size {
                        let dx = px * self.pixel_size + x_scale;
                        let dy = py * self.pixel_size + y_scale;
                        let dst_index = ((dy * image_size + dx) * 4) as usize;
                        image[dst_index..dst_index + 4].copy_from_slice(&rgba);
                    }
                }
            }
        }
        Some(DropperFrame {
            bounds,
            cursor,
            rgb,
            image_size,
            image: Arc::new(image),
        })
    }
}

impl Drop for ScreenSampler {
    fn drop(&mut self) {
        use std::ptr::null_mut;
        use windows_sys::Win32::Graphics::Gdi::{DeleteDC, DeleteObject, ReleaseDC, SelectObject};

        unsafe {
            let _ = SelectObject(self.memory_dc, self.previous);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.memory_dc);
            let _ = ReleaseDC(null_mut(), self.screen_dc);
        }
    }
}
