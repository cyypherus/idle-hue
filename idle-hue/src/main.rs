#![windows_subsystem = "windows"]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

mod auto_update;
#[cfg(not(target_os = "windows"))]
mod dropper;

#[cfg(target_os = "windows")]
use ::winit::platform::windows::IconExtWindows;
use ::winit::window::Icon;
use arboard::Clipboard;
use auto_update::{AutoUpdater, UpdateStatus};
use color::{AlphaColor, ColorSpaceTag, Oklch, Srgb, parse_color};
use haven::winit::WinitApp;
use haven::*;
use std::array::from_fn;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use tokio::sync::Mutex;

type UiCallback = Box<dyn FnOnce(&mut State, &mut PaneState) + Send>;

#[derive(Clone, Debug)]
struct PaletteState {
    colors: [Option<[f32; 3]>; PALETTE_SIZE],
    hover: [bool; PALETTE_SIZE],
    dragging: Option<usize>,
    drag_target: PaletteDragTarget,
    drag_offset: Point,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PaletteDragTarget {
    #[default]
    None,
    Swatch(usize),
    Delete,
}

#[derive(Clone, Copy, Debug)]
struct TextPopover {
    field: usize,
    position: Point,
    cut_button: ButtonState,
    copy_button: ButtonState,
    paste_button: ButtonState,
}

impl Default for PaletteState {
    fn default() -> Self {
        Self {
            colors: from_fn(|_| None),
            hover: [false; PALETTE_SIZE],
            dragging: None,
            drag_target: PaletteDragTarget::None,
            drag_offset: Point::ZERO,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct SavedState {
    values: [f32; 3],
    dark_mode: bool,
    palette: Vec<Option<[f32; 3]>>,
}

const GRAY_0_D: Color = Color::from_rgb8(0x00, 0x00, 0x00);
const GRAY_30_D: Color = Color::from_rgb8(0x1e, 0x1e, 0x1e);
const GRAY_50_D: Color = Color::from_rgb8(0x3b, 0x3b, 0x3b);
const GRAY_70_D: Color = Color::from_rgb8(0x61, 0x61, 0x61);

const GRAY_0_L: Color = Color::from_rgb8(0xff, 0xff, 0xff);
const GRAY_30_L: Color = Color::from_rgb8(0xea, 0xe4, 0xe6);
const GRAY_50_L: Color = Color::from_rgb8(0xd9, 0xd2, 0xd4);
const GRAY_70_L: Color = Color::from_rgb8(0xb6, 0xb6, 0xb8);

enum Theme {
    Gray0,
    Gray30,
    Gray50,
    Gray70,
}

const CHANNELS: [Channel; 3] = [
    Channel {
        label: "L",
        min: 0.0,
        max: 1.0,
    },
    Channel {
        label: "C",
        min: 0.0,
        max: 0.4,
    },
    Channel {
        label: "H",
        min: 0.0,
        max: 360.0,
    },
];

struct Channel {
    label: &'static str,
    min: f32,
    max: f32,
}

fn normalize_values(values: [f32; 3]) -> [f32; 3] {
    [
        if values[0].is_nan() {
            CHANNELS[0].min
        } else {
            values[0].clamp(CHANNELS[0].min, CHANNELS[0].max)
        },
        if values[1].is_nan() {
            CHANNELS[1].min
        } else {
            values[1].clamp(CHANNELS[1].min, CHANNELS[1].max)
        },
        if values[2].is_finite() {
            values[2].rem_euclid(CHANNELS[2].max)
        } else {
            CHANNELS[2].min
        },
    ]
}

const COPY_ICON: &str = include_str!("assets/copy.svg");
const CHECKMARK_ICON: &str = include_str!("assets/checkmark.svg");
const PLUS_ICON: &str = include_str!("assets/plus.svg");
const X_ICON: &str = include_str!("assets/x.svg");

const PALETTE_WIDTH: usize = 14;
const PALETTE_HEIGHT: usize = 3;
const PALETTE_SIZE: usize = PALETTE_WIDTH * PALETTE_HEIGHT;
const PALETTE_SWATCH_SIZE: f32 = 20.0;
const PALETTE_SWATCH_GAP: f32 = 5.0;

#[cfg(test)]
const TEST_FORMAT_OVERLAY_IDS: [u64; 3] = [30_003, 30_004, 30_005];
#[cfg(test)]
const TEST_CHANNEL_SLIDER_IDS: [u64; 3] = [30_006, 30_007, 30_008];

fn btn_surface_color(btn: ButtonState, base: Color) -> Color {
    match (btn.depressed, btn.hovered) {
        (true, _) => base.map_lightness(|l| l - 0.2),
        (false, true) => base.map_lightness(|l| l - 0.1),
        (false, false) => base,
    }
}

fn btn_label_color(btn: ButtonState, base: Color) -> Color {
    match (btn.depressed, btn.hovered) {
        (true, _) => base.map_lightness(|l| l - 0.2),
        (false, true) => base.map_lightness(|l| l + 0.2),
        (false, false) => base,
    }
}

const SUN_ICON: &str = include_str!("assets/sun.svg");
const MOON_ICON: &str = include_str!("assets/moon.svg");

#[cfg(not(target_os = "windows"))]
const DROPPER_ICON: &str = include_str!("assets/dropper.svg");

struct State {
    tx: Sender<UiCallback>,
    rx: Receiver<UiCallback>,
    values: [f32; 3],
    sliders: [SliderState; 3],
    format_fields: [TextState; 3],
    copy_buttons: [ButtonState; 3],
    text_popover: Option<TextPopover>,
    dark_mode: bool,
    dark_mode_button: ButtonState,
    #[cfg(not(target_os = "windows"))]
    dropper_button: ButtonState,
    update_button: ButtonState,
    update_status: UpdateStatus,
    palette: PaletteState,
    copied: Arc<Mutex<[bool; 3]>>,
}

impl State {
    fn theme(&self, t: Theme) -> Color {
        self.theme_color_invert(t, false)
    }

    fn theme_inverted(&self, t: Theme) -> Color {
        self.theme_color_invert(t, true)
    }

    fn theme_color_invert(&self, t: Theme, invert: bool) -> Color {
        let dark = if invert {
            !self.dark_mode
        } else {
            self.dark_mode
        };
        if dark {
            match t {
                Theme::Gray0 => GRAY_0_D,
                Theme::Gray30 => GRAY_30_D,
                Theme::Gray50 => GRAY_50_D,
                Theme::Gray70 => GRAY_70_D,
            }
        } else {
            match t {
                Theme::Gray0 => GRAY_0_L,
                Theme::Gray30 => GRAY_30_L,
                Theme::Gray50 => GRAY_50_L,
                Theme::Gray70 => GRAY_70_L,
            }
        }
    }
    fn oklch(&self) -> AlphaColor<Oklch> {
        AlphaColor::new([self.values[0], self.values[1], self.values[2], 1.0])
    }

    fn display_color(&self) -> Color {
        self.oklch().convert::<Srgb>()
    }

    fn srgb(&self) -> AlphaColor<Srgb> {
        self.oklch().convert::<Srgb>()
    }

    fn format_hex(&self) -> String {
        let c = self.srgb().components;
        format!(
            "#{:02x}{:02x}{:02x}",
            (c[0].clamp(0.0, 1.0) * 255.0) as u8,
            (c[1].clamp(0.0, 1.0) * 255.0) as u8,
            (c[2].clamp(0.0, 1.0) * 255.0) as u8,
        )
    }

    fn format_rgb(&self) -> String {
        let c = self.srgb().components;
        format!(
            "rgb({}, {}, {})",
            (c[0].clamp(0.0, 1.0) * 255.0) as u8,
            (c[1].clamp(0.0, 1.0) * 255.0) as u8,
            (c[2].clamp(0.0, 1.0) * 255.0) as u8,
        )
    }

    fn format_oklch(&self) -> String {
        format!(
            "oklch({:.2} {:.3} {:.1})",
            self.values[0], self.values[1], self.values[2],
        )
    }

    fn formats(&self) -> [String; 3] {
        [self.format_hex(), self.format_rgb(), self.format_oklch()]
    }

    fn update_format_fields(&mut self) {
        let fmts = self.formats();
        for (i, fmt) in fmts.iter().enumerate() {
            if self.format_fields[i].editing {
                continue;
            }
            let val = if i == 0 {
                fmt.to_uppercase()
            } else {
                fmt.clone()
            };
            self.format_fields[i] = TextState::new(val);
        }
    }

    fn end_format_editing(&mut self, app: &mut PaneState) {
        for field in &mut self.format_fields {
            if field.editing {
                field.end_editing(app);
            }
        }
    }

    fn set_values(&mut self, values: [f32; 3], app: &mut PaneState) {
        self.values = normalize_values(values);
        self.end_format_editing(app);
        self.update_ui();
    }

    fn parse_format(&mut self, text: &str) -> bool {
        let input = text.trim();
        let parsed = parse_color(input)
            .ok()
            .or_else(|| parse_color(&format!("#{input}")).ok());
        let Some(parsed) = parsed else {
            return false;
        };
        let oklch: AlphaColor<Oklch> = match parsed.cs {
            ColorSpaceTag::Oklch => parsed.to_alpha_color(),
            _ => {
                let srgb: AlphaColor<Srgb> = parsed.to_alpha_color();
                srgb.convert()
            }
        };
        let c = oklch.components;
        self.values = normalize_values([c[0], c[1], c[2]]);
        self.update_ui();
        true
    }

    fn update_sliders(&mut self) {
        for i in 0..3 {
            self.sliders[i].value = self.values[i];
        }
    }

    fn update_ui(&mut self) {
        self.update_format_fields();
        self.update_sliders();
    }
    fn config_path() -> Option<std::path::PathBuf> {
        directories::ProjectDirs::from("com", "cyy", "idle-hue")
            .map(|p| p.config_dir().join("state.json"))
    }

    fn save_state(&self, _app: &mut PaneState) {
        let saved = SavedState {
            values: self.values,
            dark_mode: self.dark_mode,
            palette: self.palette.colors.to_vec(),
        };
        tokio::spawn(async move {
            if let Some(path) = Self::config_path() {
                if let Some(parent) = path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                if let Ok(json) = serde_json::to_string_pretty(&saved) {
                    let _ = tokio::fs::write(path, json).await;
                }
            }
        });
    }
}

impl Default for State {
    fn default() -> Self {
        let (tx, rx) = channel();
        let mut s = Self {
            tx,
            rx,
            values: [0.7, 0.15, 180.0],
            sliders: Default::default(),
            format_fields: Default::default(),
            copy_buttons: Default::default(),
            text_popover: None,
            dark_mode: true,
            dark_mode_button: Default::default(),
            #[cfg(not(target_os = "windows"))]
            dropper_button: Default::default(),
            update_button: Default::default(),
            update_status: UpdateStatus::Idle,
            palette: PaletteState::default(),
            copied: Arc::new(Mutex::new([false; 3])),
        };
        s.update_ui();
        s
    }
}

#[tokio::main]
async fn main() {
    #[cfg(target_os = "windows")]
    set_app_user_model_id();

    WinitApp::new(State::default())
        .window_icon(app_icon())
        .pane(
            PaneBuilder::new("main", view)
                .title("idle-hue")
                .inner_size(400, 420)
                .on_start(on_start)
                .on_wake(on_wake)
                .on_exit(|state, app| {
                    state.save_state(app);
                }),
        )
        .run()
}

#[cfg(target_os = "windows")]
fn set_app_user_model_id() {
    let id: Vec<u16> = "cyy.apps.idle-hue"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(id.as_ptr())
    };
    if result < 0 {
        log::error!("Failed to set Windows AppUserModelID: 0x{result:08x}");
    }
}

#[cfg(target_os = "windows")]
fn app_icon() -> Icon {
    Icon::from_resource(1, None).unwrap_or_else(|_| png_app_icon())
}

#[cfg(not(target_os = "windows"))]
fn app_icon() -> Icon {
    png_app_icon()
}

fn png_app_icon() -> Icon {
    let image = image::load_from_memory(include_bytes!("assets/icon32.png"))
        .expect("icon32.png should be a valid PNG")
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).expect("icon32.png should be a valid icon")
}

fn on_start(state: &mut State, app: &mut PaneState) {
    let tx = state.tx.clone();
    let wake = app.waker();
    tokio::spawn(async move {
        if let Some(path) = State::config_path()
            && let Ok(content) = tokio::fs::read_to_string(&path).await
            && let Ok(saved) = serde_json::from_str::<SavedState>(&content)
        {
            tx.send(Box::new(move |state: &mut State, app: &mut PaneState| {
                state.set_values(saved.values, app);
                state.dark_mode = saved.dark_mode;
                for (i, color) in saved.palette.into_iter().enumerate() {
                    if i < PALETTE_SIZE {
                        state.palette.colors[i] = color.map(normalize_values);
                    }
                }
                app.redraw();
            }))
            .ok();
            wake.wake();
        }
    });

    let tx = state.tx.clone();
    let wake = app.waker();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60 * 60 * 4));
        loop {
            interval.tick().await;
            let updater = AutoUpdater::new();
            let tx = tx.clone();
            let wake = wake.clone();
            updater
                .check_and_install_updates_with_callback(Some(move |new_status: UpdateStatus| {
                    let tx = tx.clone();
                    let wake = wake.clone();
                    async move {
                        tx.send(Box::new(move |state: &mut State, app: &mut PaneState| {
                            state.update_status = new_status;
                            app.redraw();
                        }))
                        .ok();
                        wake.wake();
                    }
                }))
                .await;
        }
    });
}

fn on_wake(state: &mut State, app: &mut PaneState) {
    while let Ok(callback) = state.rx.try_recv() {
        callback(state, app);
    }
}

fn view<'a>(s: &'a State, app: &mut PaneState) -> View<'a, State> {
    let bg = s.theme(Theme::Gray0);
    let field_bg = s.theme(Theme::Gray30);
    let field_border = s.theme(Theme::Gray50);
    let label_color = s.theme_inverted(Theme::Gray0);
    let highlight_color = {
        let luminance = s.display_color().discard_alpha().relative_luminance();
        let range = if s.dark_mode { 0.1..1. } else { 0.0..0.9 };
        if !range.contains(&luminance) {
            s.theme_inverted(Theme::Gray0)
        } else {
            s.display_color()
        }
    }
    .with_alpha(0.5);

    stack(vec![
        rect(id!()).fill(bg).corner_rounding(0.).build(app),
        column(vec![
            column_spaced(
                10.,
                vec![
                    row_spaced(10., {
                        let mut buttons: Vec<View<'_, State>> = vec![space().inert_y()];
                        #[cfg(not(target_os = "windows"))]
                        buttons.push(
                            button(
                                id!(),
                                (
                                    &s.dropper_button,
                                    Binding::new(
                                        |s: &State| &s.dropper_button,
                                        |s: &mut State| &mut s.dropper_button,
                                    ),
                                ),
                            )
                            .surface(move |btn, ctx| {
                                rect(id!())
                                    .fill(btn_surface_color(btn, field_bg))
                                    .stroke(field_border, Stroke::new(1.))
                                    .corner_rounding(7.)
                                    .build(ctx)
                            })
                            .label(move |btn, ctx| {
                                svg(id!(), DROPPER_ICON)
                                    .fill(Brush::Solid(btn_label_color(btn, label_color)))
                                    .finish(ctx)
                                    .pad(6.)
                            })
                            .on_click(|state, app| {
                                let tx = state.tx.clone();
                                let wake = app.waker();
                                tokio::spawn(async move {
                                    if let Ok(Some(rgb)) = dropper::sample_color().await {
                                        tx.send(Box::new(
                                            move |state: &mut State, app: &mut PaneState| {
                                                let srgb = AlphaColor::<Srgb>::new([
                                                    rgb[0], rgb[1], rgb[2], 1.0,
                                                ]);
                                                let oklch: AlphaColor<Oklch> = srgb.convert();
                                                let c = oklch.components;
                                                state.set_values([c[0], c[1], c[2]], app);
                                                app.redraw();
                                            },
                                        ))
                                        .ok();
                                        wake.wake();
                                    }
                                });
                            })
                            .build(app)
                            .height(30.)
                            .width(30.),
                        );
                        buttons.push(
                            button(
                                id!(),
                                (
                                    &s.dark_mode_button,
                                    Binding::new(
                                        |s: &State| &s.dark_mode_button,
                                        |s: &mut State| &mut s.dark_mode_button,
                                    ),
                                ),
                            )
                            .surface(move |btn, ctx| {
                                rect(id!())
                                    .fill(btn_surface_color(btn, field_bg))
                                    .stroke(field_border, Stroke::new(1.))
                                    .corner_rounding(7.)
                                    .build(ctx)
                            })
                            .label(move |btn, ctx| {
                                svg(id!(), if s.dark_mode { SUN_ICON } else { MOON_ICON })
                                    .fill(Brush::Solid(btn_label_color(btn, label_color)))
                                    .finish(ctx)
                                    .pad(6.)
                            })
                            .on_click(|state, _| {
                                state.dark_mode = !state.dark_mode;
                            })
                            .build(app)
                            .height(30.)
                            .width(30.),
                        );
                        buttons
                    }),
                    row_spaced(
                        10.,
                        vec![
                            rect(id!())
                                .fill(s.display_color())
                                .stroke(field_border, Stroke::new(1.))
                                .corner_rounding(8.)
                                .build(app)
                                .inert_y()
                                .aspect_width(1.),
                            row_spaced(
                                10.,
                                vec![
                                    column_spaced(
                                        10.,
                                        (0..3)
                                            .map(|i| {
                                                let field_id = id!(i as u64);
                                                #[cfg(test)]
                                                let overlay_id = TEST_FORMAT_OVERLAY_IDS[i];
                                                #[cfg(not(test))]
                                                let overlay_id = id!(100 + i as u64);
                                                stack(vec![
                                                    text_field(
                                                        field_id,
                                                        (
                                                            &s.format_fields[i],
                                                            Binding::new(
                                                                move |s: &State| {
                                                                    &s.format_fields[i]
                                                                },
                                                                move |s: &mut State| {
                                                                    &mut s.format_fields[i]
                                                                },
                                                            ),
                                                        ),
                                                    )
                                                    .font_size(16)
                                                    .text_fill(label_color)
                                                    .cursor_fill(label_color)
                                                    .highlight_fill(highlight_color)
                                                    .singleline()
                                                    .enter_end_editing()
                                                    .esc_end_editing()
                                                    .on_edit(move |state, _app, edit| match edit {
                                                        EditInteraction::Start => {}
                                                        EditInteraction::Update(text) => {
                                                            state.parse_format(&text);
                                                        }
                                                        EditInteraction::End => {
                                                            state.update_ui();
                                                        }
                                                    })
                                                    .background(move |_, _, ctx| {
                                                        rect(id!(i as u64))
                                                            .fill(s.theme(Theme::Gray30))
                                                            .stroke(field_border, Stroke::new(1.))
                                                            .corner_rounding(6.)
                                                            .build(ctx)
                                                    })
                                                    .padding(5.)
                                                    .build(app)
                                                    .expand_x()
                                                    .height(30.),
                                                    rect(overlay_id)
                                                        .fill(Color::TRANSPARENT)
                                                        .view()
                                                        .gesture(
                                                            gesture::click(id!(i as u64))
                                                                .button(MouseButton::Right)
                                                                .run(move |state: &mut State, app, event| match event.state {
                                                                    ClickPhase::Started => {
                                                                        if !state.format_fields[i]
                                                                            .editing
                                                                        {
                                                                            state.format_fields[i]
                                                                                .begin_editing_with(
                                                                                    app,
                                                                                    InitialSelection::All,
                                                                                );
                                                                        }
                                                                    }
                                                                    ClickPhase::Completed => {
                                                                        state.text_popover =
                                                                            Some(TextPopover {
                                                                                field: i,
                                                                                position: event
                                                                                    .location
                                                                                    .global(),
                                                                                cut_button:
                                                                                    ButtonState::default(),
                                                                                copy_button:
                                                                                    ButtonState::default(),
                                                                                paste_button:
                                                                                    ButtonState::default(),
                                                                            });
                                                                    }
                                                                    ClickPhase::Cancelled => {}
                                                                }),
                                                        )
                                                        .build(app)
                                                        .expand_x()
                                                        .height(30.),
                                                ])
                                                .expand_x()
                                                .height(30.)
                                            })
                                            .collect(),
                                    )
                                    .expand_x(),
                                    column_spaced(
                                        10.,
                                        (0..3)
                                            .map(|i| {
                                                let copied = s
                                                    .copied
                                                    .try_lock()
                                                    .map(|c| c[i])
                                                    .unwrap_or(false);
                                                let copied_state = s.copied.clone();
                                                button(
                                                    id!(i as u64),
                                                    (
                                                        &s.copy_buttons[i],
                                                        Binding::new(
                                                            move |s: &State| &s.copy_buttons[i],
                                                            move |s: &mut State| {
                                                                &mut s.copy_buttons[i]
                                                            },
                                                        ),
                                                    ),
                                                )
                                                .surface(move |btn, ctx| {
                                                    rect(id!(i as u64))
                                                        .fill(btn_surface_color(
                                                            btn,
                                                            s.theme(Theme::Gray30),
                                                        ))
                                                        .stroke(field_border, Stroke::new(1.))
                                                        .corner_rounding(6.)
                                                        .build(ctx)
                                                })
                                                .label(move |btn, ctx| {
                                                    let c = btn_label_color(btn, label_color);
                                                    if copied {
                                                        svg(id!(i as u64), CHECKMARK_ICON)
                                                            .fill(Brush::Solid(c))
                                                            .finish(ctx)
                                                            .width(14.)
                                                            .height(14.)
                                                            .pad(5.)
                                                    } else {
                                                        svg(id!(i as u64), COPY_ICON)
                                                            .fill(Brush::Solid(c))
                                                            .finish(ctx)
                                                            .width(14.)
                                                            .height(14.)
                                                            .pad(5.)
                                                    }
                                                })
                                                .on_click(move |state, app| {
                                                    let text = state.formats()[i].clone();
                                                    if let Ok(mut cb) = Clipboard::new() {
                                                        let _ = cb.set_text(text);
                                                    }
                                                    if let Ok(mut c) = copied_state.try_lock() {
                                                        c[i] = true;
                                                    }
                                                    let copied_reset = copied_state.clone();
                                                    let wake = app.waker();
                                                    tokio::spawn(async move {
                                                        tokio::time::sleep(
                                                            tokio::time::Duration::from_secs(2),
                                                        )
                                                        .await;
                                                        copied_reset.lock().await[i] = false;
                                                        wake.wake();
                                                    });
                                                })
                                                .build(app)
                                                .width(30.)
                                                .height(30.)
                                            })
                                            .collect(),
                                    ),
                                ],
                            ),
                        ],
                    ),
                    row_spaced(
                        10.,
                        vec![
                            column_spaced(
                                8.,
                                (0..3)
                                    .map(|i| {
                                        stack(vec![
                                            text(id!(i as u64), CHANNELS[i].label)
                                                .font_size(16)
                                                .font_weight(FontWeight::BOLD)
                                                .fill(label_color)
                                                .build(app),
                                        ])
                                        .height(30.)
                                    })
                                    .collect(),
                            )
                            .width(20.),
                            column_spaced(
                                8.,
                                (0..3)
                                    .map(|i| {
                                        channel_slider(
                                            id!(i as u64),
                                            i,
                                            binding!(s.sliders),
                                            s.values,
                                            s.theme_inverted(Theme::Gray0),
                                            app,
                                        )
                                    })
                                    .collect(),
                            )
                            .width_range(200.0..),
                        ],
                    ),
                    palette_grid(s, app),
                ],
            )
            .pad_x(20.)
            .pad_top(20.)
            .pad_bottom(10.)
            .expand_y(),
            space(),
            rect(id!())
                .fill(field_border)
                .corner_rounding(0.)
                .build(app)
                .height(1.),
            row(vec![update_button(s, label_color, app), space().inert_y()])
                .pad_x(20.)
                .pad_y(6.),
        ]),
        text_popover_layer(s, field_bg, field_border, label_color, app),
    ])
}

fn text_popover_layer<'a>(
    s: &'a State,
    field_bg: Color,
    field_border: Color,
    label_color: Color,
    app: &mut PaneState,
) -> View<'a, State> {
    let Some(popover) = s.text_popover.as_ref() else {
        return empty();
    };
    let field = popover.field;
    let x = popover.position.x as f32;
    let y = popover.position.y as f32;

    let cut_button = context_menu_button(
        context_menu_action_id(field, 0),
        "Cut",
        "X",
        (
            &popover.cut_button,
            Binding::new(
                |s: &State| &s.text_popover.as_ref().unwrap().cut_button,
                |s: &mut State| &mut s.text_popover.as_mut().unwrap().cut_button,
            ),
        ),
        field_bg,
        label_color,
        app,
        move |state, app| {
            if state.format_fields[field].cut_text(app).is_some() {
                let text = state.format_fields[field].text.clone();
                state.parse_format(&text);
            }
        },
    );

    let copy_button = context_menu_button(
        context_menu_action_id(field, 1),
        "Copy",
        "C",
        (
            &popover.copy_button,
            Binding::new(
                |s: &State| &s.text_popover.as_ref().unwrap().copy_button,
                |s: &mut State| &mut s.text_popover.as_mut().unwrap().copy_button,
            ),
        ),
        field_bg,
        label_color,
        app,
        move |state, _app| {
            let _ = state.format_fields[field].copy_text();
        },
    );

    let paste_button = context_menu_button(
        context_menu_action_id(field, 2),
        "Paste",
        "V",
        (
            &popover.paste_button,
            Binding::new(
                |s: &State| &s.text_popover.as_ref().unwrap().paste_button,
                |s: &mut State| &mut s.text_popover.as_mut().unwrap().paste_button,
            ),
        ),
        field_bg,
        label_color,
        app,
        move |state, app| {
            state.format_fields[field].paste_text(app);
        },
    );

    let close_inside = gesture::click(id!())
        .button(MouseButton::Left)
        .observe()
        .run(|state: &mut State, _app, event| {
            if matches!(event.state, ClickPhase::Completed) {
                state.text_popover = None;
            }
        });
    let close_outside = gesture::click(id!())
        .anywhere()
        .button(MouseButton::Left)
        .observe()
        .run(|state: &mut State, _app, event| {
            if matches!(event.state, ClickPhase::Completed) {
                state.text_popover = None;
            }
        });
    let close_drag_outside = gesture::drag(id!())
        .anywhere()
        .button(MouseButton::Left)
        .observe()
        .run(|state: &mut State, _app, event| {
            if matches!(event, DragPhase::Completed { .. }) {
                state.text_popover = None;
            }
        });

    stack(vec![
        stack(vec![
            shadow(id!()).build(app).offset(0., 5.),
            rect(id!())
                .fill(field_bg)
                .stroke(field_border, Stroke::new(1.))
                .corner_rounding(6.)
                .view()
                .gesture(close_inside)
                .occlude(&close_outside)
                .occlude(&close_drag_outside)
                .build(app),
            column_spaced(2., vec![cut_button, copy_button, paste_button]).pad(3.),
        ])
        .align(Align::TopLeading),
    ])
    .width(1.)
    .height(1.)
    .offset(x, y)
    .align(Align::TopLeading)
    .layer(10)
}

fn context_menu_action_id(field: usize, action: u64) -> u64 {
    ((field as u64) << 8) | action
}

fn context_menu_button<'a>(
    action_id: u64,
    label: &'static str,
    shortcut_key: &'static str,
    state: (&'a ButtonState, Binding<State, ButtonState>),
    field_bg: Color,
    label_color: Color,
    app: &mut PaneState,
    on_click: impl Fn(&mut State, &mut PaneState) + 'static,
) -> View<'a, State> {
    button(id!(action_id, 0_u64), state)
        .surface(move |btn, ctx| {
            rect(id!(action_id, 1_u64))
                .fill(btn_surface_color(btn, field_bg))
                .corner_rounding(5.)
                .build(ctx)
        })
        .label(move |btn, ctx| {
            row(vec![
                text(id!(action_id, 2_u64), label)
                    .font_size(13)
                    .fill(btn_label_color(btn, label_color))
                    .build(ctx),
                space().inert_y(),
                shortcut_view(
                    action_id,
                    shortcut_key,
                    btn_label_color(btn, label_color),
                    ctx,
                ),
            ])
            .pad_x(8.)
        })
        .on_click(move |state, app| {
            on_click(state, app);
            state.text_popover = None;
        })
        .build(app)
        .width(90.)
        .height(28.)
}

fn shortcut_view<'a>(
    action_id: u64,
    key: &'static str,
    color: Color,
    app: &mut PaneState,
) -> View<'a, State> {
    let color = color.with_alpha(0.7);
    if cfg!(target_os = "macos") {
        row_spaced(
            3.,
            vec![
                text(id!(action_id, 3_u64), "⌘")
                    .font_family("Apple Symbols")
                    .font_size(12)
                    .fill(color)
                    .build(app),
                text(id!(action_id, 4_u64), key)
                    .font_size(12)
                    .fill(color)
                    .build(app),
            ],
        )
    } else {
        text(id!(action_id, 5_u64), format!("Ctrl {key}"))
            .font_size(12)
            .fill(color)
            .build(app)
    }
}

fn update_button<'a>(s: &'a State, label_color: Color, app: &mut PaneState) -> View<'a, State> {
    let status = &s.update_status;
    let btn = s.update_button;
    let label_text = match status {
        UpdateStatus::Idle if btn.hovered => "check for updates".to_string(),
        UpdateStatus::Idle => format!("idle-hue {}", env!("CARGO_PKG_VERSION")),
        UpdateStatus::Checking => "checking...".to_string(),
        UpdateStatus::Downloading { .. } => "downloading...".to_string(),
        UpdateStatus::Installing { .. } => "installing...".to_string(),
        UpdateStatus::Updated { .. } => "restart to update".to_string(),
        UpdateStatus::UpToDate { .. } => "up to date :)".to_string(),
        UpdateStatus::Error(msg) => {
            if msg.len() > 30 {
                format!("{}...", &msg[..27])
            } else {
                msg.clone()
            }
        }
    };
    let gray = s.theme(Theme::Gray70);
    button(id!(), binding!(s.update_button))
        .surface(move |_, _ctx| space().height(0.).width(0.))
        .label(move |btn, ctx| {
            let c = if btn.hovered { label_color } else { gray };
            text(id!(), &label_text).font_size(13).fill(c).build(ctx)
        })
        .on_click(move |state, app| {
            if matches!(state.update_status, UpdateStatus::Updated { .. }) {
                tokio::spawn(async move {
                    if let Err(e) = AutoUpdater::restart_application().await {
                        log::error!("Failed to restart: {e}");
                    }
                });
            } else if !matches!(state.update_status, UpdateStatus::Checking) {
                state.update_status = UpdateStatus::Checking;
                app.redraw();
                let tx = state.tx.clone();
                let wake = app.waker();
                tokio::spawn(async move {
                    let updater = AutoUpdater::new();
                    updater
                        .check_and_install_updates_with_callback(Some(
                            move |new_status: UpdateStatus| {
                                let tx = tx.clone();
                                let wake = wake.clone();
                                async move {
                                    tx.send(Box::new(
                                        move |state: &mut State, app: &mut PaneState| {
                                            state.update_status = new_status;
                                            app.redraw();
                                        },
                                    ))
                                    .ok();
                                    wake.wake();
                                }
                            },
                        ))
                        .await;
                });
            }
        })
        .build(app)
        .height(25.)
}

fn channel_slider<'a>(
    key: u64,
    i: usize,
    binding: (&[SliderState; 3], Binding<State, [SliderState; 3]>),
    values: [f32; 3],
    knob_color: Color,
    app: &mut PaneState,
) -> View<'a, State> {
    let ch = &CHANNELS[i];
    #[cfg(test)]
    let slider_id = TEST_CHANNEL_SLIDER_IDS[i];
    #[cfg(not(test))]
    let slider_id = id!(key);
    let stops: Vec<Color> = (0..=16)
        .map(|step| {
            let t = step as f32 / 16.0;
            let val = ch.min + t * (ch.max - ch.min);
            let mut v = values;
            v[i] = val;
            let oklch = AlphaColor::<Oklch>::new([v[0], v[1], v[2], 1.0]);
            oklch.convert::<Srgb>()
        })
        .collect();
    slider(
        slider_id,
        (
            &binding.0[i],
            Binding::new(
                move |s: &State| &s.sliders[i],
                move |s: &mut State| &mut s.sliders[i],
            ),
        ),
    )
    .range(ch.min, ch.max)
    .background(move |_, area, ctx| {
        rect(id!(key))
            .fill(
                Gradient::new_linear(
                    (area.x as f64, area.y as f64),
                    (area.x as f64 + area.width as f64, area.y as f64),
                )
                .with_stops(stops.as_slice()),
            )
            .corner_rounding(area.height)
            .build(ctx)
    })
    .track(move |_, area, ctx| {
        rect(id!(key))
            .fill(Color::TRANSPARENT)
            .corner_rounding(area.height)
            .build(ctx)
    })
    .traveled_track(move |_, area, ctx| {
        rect(id!(key))
            .fill(Color::TRANSPARENT)
            .corner_rounding(area.height)
            .build(ctx)
    })
    .knob(move |state, _, ctx| {
        circle(id!(key))
            .fill(btn_label_color(
                ButtonState {
                    depressed: state.dragging,
                    hovered: state.hovered,
                },
                knob_color,
            ))
            .finish(ctx)
    })
    .on_change(move |state, app, val| {
        let mut values = state.values;
        values[i] = val;
        state.set_values(values, app);
    })
    .build(app)
    .height(26.)
    .pad_y(2.)
}

fn palette_color(values: [f32; 3]) -> Color {
    AlphaColor::<Oklch>::new([values[0], values[1], values[2], 1.0]).convert::<Srgb>()
}

fn palette_grid<'a>(s: &'a State, app: &mut PaneState) -> View<'a, State> {
    let rows = (0..PALETTE_HEIGHT)
        .map(|row| {
            let cols = (0..PALETTE_WIDTH)
                .map(|col| {
                    let index = row * PALETTE_WIDTH + col;
                    let swatch_color = s.palette.colors[index].map(palette_color);
                    let is_dragging_this = s.palette.dragging == Some(index);
                    let is_dragging = s.palette.dragging.is_some();
                    let is_drag_target = matches!(
                        s.palette.drag_target,
                        PaletteDragTarget::Swatch(target) if target == index
                    ) && is_dragging;

                    stack(vec![
                        palette_swatch(
                            index,
                            swatch_color,
                            is_dragging_this,
                            is_drag_target,
                            s.palette.drag_target == PaletteDragTarget::Delete,
                            s,
                            app,
                        ),
                        palette_sensor(index, app),
                        svg(id!(index as u64), PLUS_ICON)
                            .fill(
                                if swatch_color.is_none() && s.palette.hover[index] && !is_dragging
                                {
                                    s.theme_inverted(Theme::Gray0)
                                } else {
                                    TRANSPARENT
                                },
                            )
                            .finish(app)
                            .height(15.)
                            .width(15.),
                    ])
                    .height(PALETTE_SWATCH_SIZE)
                    .width(PALETTE_SWATCH_SIZE)
                })
                .collect::<Vec<_>>();

            row_spaced(PALETTE_SWATCH_GAP, cols)
        })
        .collect::<Vec<_>>();

    stack(vec![
        rect(id!())
            .fill(Color::TRANSPARENT)
            .view()
            .gesture(gesture::hover(id!()).observe().run(
                move |state: &mut State, _app, hovered| {
                    if state.palette.dragging.is_some() {
                        state.palette.drag_target = if hovered {
                            PaletteDragTarget::None
                        } else {
                            PaletteDragTarget::Delete
                        };
                    }
                },
            ))
            .build(app)
            .inert(),
        column_spaced(PALETTE_SWATCH_GAP, rows),
    ])
}

fn palette_swatch<'a>(
    index: usize,
    swatch_color: Option<Color>,
    is_dragging: bool,
    is_drag_target: bool,
    is_delete_target: bool,
    s: &'a State,
    app: &mut PaneState,
) -> View<'a, State> {
    let swatch = stack(vec![
        rect(id!(index as u64))
            .fill(swatch_color.unwrap_or(s.theme(Theme::Gray30)))
            .stroke(
                if is_drag_target {
                    s.theme_inverted(Theme::Gray0)
                } else {
                    s.theme(Theme::Gray50)
                },
                Stroke::new(if is_dragging {
                    3.
                } else if is_drag_target {
                    2.
                } else {
                    1.
                }),
            )
            .corner_rounding(6.)
            .build(app),
        stack(vec![
            rect(id!(index as u64))
                .corner_rounding(4.)
                .fill(if is_delete_target && is_dragging {
                    s.theme(Theme::Gray30).with_alpha(0.5)
                } else {
                    TRANSPARENT
                })
                .build(app)
                .inert(),
            svg(id!(index as u64), X_ICON)
                .fill(if is_delete_target && is_dragging {
                    s.theme_inverted(Theme::Gray0)
                } else {
                    TRANSPARENT
                })
                .finish(app),
        ])
        .height(15.)
        .width(15.)
        .inert(),
    ])
    .offset(
        if is_dragging {
            s.palette.drag_offset.x as f32
        } else {
            0.0
        },
        if is_dragging {
            s.palette.drag_offset.y as f32
        } else {
            0.0
        },
    );
    if is_dragging { swatch.layer(1) } else { swatch }
}

fn palette_sensor(index: usize, app: &mut PaneState) -> View<'static, State> {
    let id = index as u64;
    rect(id!(index as u64))
        .fill(Color::TRANSPARENT)
        .view()
        .gesture(gesture::hover(id!(id, 0_u64)).observe().run(
            move |state: &mut State, _app, hovered| {
                state.palette.hover[index] = hovered;
                if state.palette.dragging.is_some() {
                    if hovered {
                        state.palette.drag_target = PaletteDragTarget::Swatch(index);
                    } else if state.palette.drag_target == PaletteDragTarget::Swatch(index) {
                        state.palette.drag_target = PaletteDragTarget::None;
                    }
                }
            },
        ))
        .gesture(
            gesture::click(id!(id, 1_u64))
                .button(MouseButton::Left)
                .run(move |state: &mut State, app, event| {
                    if matches!(event.state, ClickPhase::Completed) {
                        if let Some(palette_values) = state.palette.colors[index] {
                            state.set_values(palette_values, app);
                        } else {
                            state.palette.colors[index] = Some(state.values);
                        }
                        state.save_state(app);
                    }
                }),
        )
        .gesture(gesture::drag(id!(id, 2_u64)).button(MouseButton::Left).run(
            move |state: &mut State, app, drag| match drag {
                DragPhase::Began { .. } => {
                    if state.palette.colors[index].is_some() {
                        state.palette.dragging = Some(index);
                        state.palette.drag_offset = Point::ZERO;
                        state.palette.drag_target = PaletteDragTarget::Swatch(index);
                    }
                }
                DragPhase::Updated { start, current, .. } => {
                    if state.palette.dragging == Some(index) {
                        state.palette.drag_offset =
                            Point::new(current.x - start.x, current.y - start.y);
                    }
                }
                DragPhase::Completed { .. } => {
                    if let Some(dragging_index) = state.palette.dragging {
                        let changed = match state.palette.drag_target {
                            PaletteDragTarget::Swatch(target_index) => {
                                let dragging_color = state.palette.colors[dragging_index];
                                let target_color = state.palette.colors[target_index];
                                state.palette.colors[dragging_index] = target_color;
                                state.palette.colors[target_index] = dragging_color;
                                true
                            }
                            PaletteDragTarget::Delete => {
                                state.palette.colors[dragging_index] = None;
                                true
                            }
                            PaletteDragTarget::None => false,
                        };
                        if changed {
                            state.save_state(app);
                        }
                        state.palette.dragging = None;
                        state.palette.drag_target = PaletteDragTarget::None;
                        state.palette.drag_offset = Point::ZERO;
                    }
                }
            },
        ))
        .build(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn right_click_format(pane: &mut Pane<State>, state: &mut State, index: usize) {
        let location = pane.location(TEST_FORMAT_OVERLAY_IDS[index]).unwrap();
        pane.move_to(state, location);
        pane.press_button(state, MouseButton::Right);
        pane.release_button(state, MouseButton::Right);
        pane.redraw(state, 520, 360, 1.0);
    }

    #[test]
    fn idle_hue_format_right_click_focus_rotates_after_each_field_has_been_focused() {
        let mut state = State::default();
        let mut pane = PaneBuilder::new("test", view).build();
        pane.redraw(&mut state, 520, 360, 1.0);

        for index in [0, 1, 2, 0] {
            right_click_format(&mut pane, &mut state, index);
            assert!(state.format_fields[index].editing);
        }

        pane.key_pressed(&mut state, "x");
        assert_eq!(state.format_fields[0].text, "x");
    }

    #[test]
    fn color_update_clears_app_format_editing_state() {
        let mut state = State::default();
        let mut pane = PaneBuilder::new("test", view).build();
        pane.redraw(&mut state, 520, 360, 1.0);

        for field in &mut state.format_fields {
            field.editing = true;
        }

        let slider_location = pane.location(TEST_CHANNEL_SLIDER_IDS[0]).unwrap();
        pane.click(
            &mut state,
            Point::new(slider_location.x - 80., slider_location.y),
        );

        assert_ne!(state.values[0], 0.7);
        assert!(state.format_fields.iter().all(|field| !field.editing));
    }

    #[test]
    fn parsed_format_update_keeps_format_field_editing() {
        let mut state = State::default();
        state.format_fields[1].editing = true;

        assert!(state.parse_format("red"));

        assert_ne!(state.values, [0.7, 0.15, 180.0]);
        assert!(state.format_fields[1].editing);
    }

    #[test]
    fn parsed_oklch_values_stay_inside_slider_ranges() {
        let parsed = parse_color("oklch(0.7 4.2 900)").unwrap();
        let oklch: AlphaColor<Oklch> = parsed.to_alpha_color();
        let c = oklch.components;

        assert_eq!(normalize_values([c[0], c[1], c[2]]), [0.7, 0.4, 180.0]);
    }

    #[test]
    fn direct_color_updates_stay_inside_slider_ranges() {
        assert_eq!(
            normalize_values([f32::NAN, f32::INFINITY, -30.0]),
            [0.0, 0.4, 330.0],
        );
    }
}
