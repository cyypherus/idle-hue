#![windows_subsystem = "windows"]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

mod auto_update;
mod dropper;

use arboard::Clipboard;
use auto_update::{AutoUpdater, UpdateStatus};
use color::{AlphaColor, ColorSpaceTag, Oklch, Srgb, parse_color};
use haven::*;
use std::sync::Arc;
use tokio::sync::Mutex;

const GRAY_0_D: Color = Color::from_rgb8(0x00, 0x00, 0x00);
const GRAY_30_D: Color = Color::from_rgb8(0x1e, 0x1e, 0x1e);
const GRAY_50_D: Color = Color::from_rgb8(0x3b, 0x3b, 0x3b);
const GRAY_70_D: Color = Color::from_rgb8(0x61, 0x61, 0x61);

const GRAY_0_L: Color = Color::from_rgb8(0xff, 0xff, 0xff);
const GRAY_30_L: Color = Color::from_rgb8(0xea, 0xe4, 0xe6);
const GRAY_50_L: Color = Color::from_rgb8(0xd9, 0xd2, 0xd4);
const GRAY_70_L: Color = Color::from_rgb8(0xb6, 0xb6, 0xb8);

#[allow(dead_code)]
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

const COPY_ICON: &str = include_str!("assets/copy.svg");
const CHECKMARK_ICON: &str = include_str!("assets/checkmark.svg");

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

const DROPPER_ICON: &str = include_str!("assets/dropper.svg");

struct State {
    values: [f32; 3],
    sliders: [SliderState; 3],
    format_fields: [TextState; 3],
    editing_format: Option<usize>,
    copy_buttons: [ButtonState; 3],
    dark_mode: bool,
    dark_mode_button: ButtonState,
    dropper_button: ButtonState,
    update_button: ButtonState,
    update_status: UpdateStatus,
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
        for i in 0..3 {
            if self.editing_format == Some(i) {
                continue;
            }
            let val = if i == 0 {
                fmts[i].to_uppercase()
            } else {
                fmts[i].clone()
            };
            self.format_fields[i] = TextState::new(val);
        }
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
        self.values = [c[0], c[1], c[2]];
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
}

impl Default for State {
    fn default() -> Self {
        let mut s = Self {
            values: [0.7, 0.15, 180.0],
            sliders: Default::default(),
            format_fields: Default::default(),
            editing_format: None,
            copy_buttons: Default::default(),
            dark_mode: true,
            dark_mode_button: Default::default(),
            dropper_button: Default::default(),
            update_button: Default::default(),
            update_status: UpdateStatus::Idle,
            copied: Arc::new(Mutex::new([false; 3])),
        };
        s.update_ui();
        s
    }
}

fn main() {
    App::builder(
        State::default(),
        Window::new("main", view)
            .title("idle-hue")
            .inner_size(400, 350),
    )
    .on_start(|_state, app| {
        let tx = app.callback(|state: &mut State, status: UpdateStatus| {
            state.update_status = status;
        });
        app.spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60 * 60 * 4));
            loop {
                interval.tick().await;
                let updater = AutoUpdater::new();
                let tx = tx.clone();
                updater
                    .check_and_install_updates_with_callback(Some(
                        move |new_status: UpdateStatus| {
                            let tx = tx.clone();
                            async move {
                                tx.send(new_status);
                            }
                        },
                    ))
                    .await;
            }
        });
    })
    .start()
}

fn view<'a>(s: &'a State, app: &mut AppState) -> Layout<'a, View<State>, AppCtx> {
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
        rect(id!()).fill(bg).corner_rounding(0.).build(app.ctx()),
        column(vec![
            column_spaced(
                10.,
                vec![
                    row_spaced(
                        10.,
                        vec![
                            space().inert_y(),
                            button(
                                id!(),
                                (
                                    s.dropper_button,
                                    Binding::new(
                                        |s: &State| s.dropper_button,
                                        |s: &mut State, v| s.dropper_button = v,
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
                            .on_click(|_state, app: &mut AppState| {
                                let tx = app.callback(|state: &mut State, rgb: [f32; 3]| {
                                    let srgb =
                                        AlphaColor::<Srgb>::new([rgb[0], rgb[1], rgb[2], 1.0]);
                                    let oklch: AlphaColor<Oklch> = srgb.convert();
                                    let c = oklch.components;
                                    state.values = [c[0], c[1], c[2]];
                                    state.update_ui();
                                });
                                app.spawn(async move {
                                    if let Ok(Some(rgb)) = dropper::sample_color().await {
                                        tx.send(rgb);
                                    }
                                });
                            })
                            .build(app.ctx())
                            .height(30.)
                            .width(30.),
                            button(
                                id!(),
                                (
                                    s.dark_mode_button,
                                    Binding::new(
                                        |s: &State| s.dark_mode_button,
                                        |s: &mut State, v| s.dark_mode_button = v,
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
                            .build(app.ctx())
                            .height(30.)
                            .width(30.),
                        ],
                    ),
                    row_spaced(
                        10.,
                        vec![
                            rect(id!())
                                .fill(s.display_color())
                                .stroke(field_border, Stroke::new(1.))
                                .corner_rounding(8.)
                                .build(app.ctx())
                                .inert_y(),
                            row_spaced(
                                10.,
                                vec![
                                    column_spaced(
                                        10.,
                                        (0..3)
                                            .map(|i| {
                                                text_field(
                                                    id!(i as u64),
                                                    (
                                                        s.format_fields[i].clone(),
                                                        Binding::new(
                                                            move |s: &State| {
                                                                s.format_fields[i].clone()
                                                            },
                                                            move |s: &mut State, v| {
                                                                s.format_fields[i] = v
                                                            },
                                                        ),
                                                    ),
                                                )
                                                .font_size(16)
                                                .text_fill(label_color)
                                                .cursor_fill(label_color)
                                                .highlight_fill(highlight_color)
                                                .enter_end_editing()
                                                .esc_end_editing()
                                                .on_edit(move |state, _, edit| match edit {
                                                    EditInteraction::Update(text) => {
                                                        state.editing_format = Some(i);
                                                        if state.parse_format(&text) {
                                                            state.update_ui();
                                                        }
                                                    }
                                                    EditInteraction::End => {
                                                        state.editing_format = None;
                                                        state.update_ui();
                                                    }
                                                })
                                                .background(move |_, _, ctx| {
                                                    rect(id!(i as u64))
                                                        .fill(s.theme(Theme::Gray30))
                                                        .stroke(s.display_color(), Stroke::new(1.))
                                                        .corner_rounding(6.)
                                                        .build(ctx)
                                                })
                                                .padding(5.)
                                                .build(app.ctx())
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
                                                        s.copy_buttons[i],
                                                        Binding::new(
                                                            move |s: &State| s.copy_buttons[i],
                                                            move |s: &mut State, v| {
                                                                s.copy_buttons[i] = v
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
                                                        .stroke(s.display_color(), Stroke::new(1.))
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
                                                    let redraw = app.redraw_trigger();
                                                    app.spawn(async move {
                                                        tokio::time::sleep(
                                                            tokio::time::Duration::from_secs(2),
                                                        )
                                                        .await;
                                                        copied_reset.lock().await[i] = false;
                                                        redraw.trigger().await;
                                                    });
                                                })
                                                .build(app.ctx())
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
                                                .build(app.ctx()),
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
                                            binding!(s, State, sliders),
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
                ],
            )
            .pad(20.)
            .expand_y(),
            space(),
            rect(id!())
                .fill(field_border)
                .corner_rounding(0.)
                .build(app.ctx())
                .height(1.),
            row(vec![update_button(s, label_color, app), space().inert_y()])
                .pad_x(20.)
                .pad_y(6.),
        ]),
    ])
}

fn update_button<'a>(
    s: &'a State,
    label_color: Color,
    app: &mut AppState,
) -> Layout<'a, View<State>, AppCtx> {
    let status = &s.update_status;
    let btn = s.update_button;
    let label_text = match status {
        UpdateStatus::Idle if btn.hovered => "check for updates".to_string(),
        UpdateStatus::Idle => format!("idle-hue {}", env!("CARGO_PKG_VERSION")),
        UpdateStatus::Checking => "checking...".to_string(),
        UpdateStatus::Downloading { .. } => "downloading...".to_string(),
        UpdateStatus::Installing { .. } => "installing...".to_string(),
        UpdateStatus::Updated { .. } => "restart to update".to_string(),
        UpdateStatus::UpToDate { .. } => "up to date".to_string(),
        UpdateStatus::Error(msg) => {
            if msg.len() > 30 {
                format!("{}...", &msg[..27])
            } else {
                msg.clone()
            }
        }
    };
    let gray = s.theme(Theme::Gray70);
    button(id!(), binding!(s, State, update_button))
        .surface(move |_, _ctx| space().height(0.).width(0.))
        .label(move |btn, ctx| {
            let c = if btn.hovered { label_color } else { gray };
            text(id!(), &label_text).font_size(13).fill(c).build(ctx)
        })
        .on_click(move |state, app| {
            if matches!(state.update_status, UpdateStatus::Updated { .. }) {
                app.spawn(async move {
                    if let Err(e) = AutoUpdater::restart_application().await {
                        log::error!("Failed to restart: {e}");
                    }
                });
            } else if !matches!(state.update_status, UpdateStatus::Checking) {
                state.update_status = UpdateStatus::Checking;
                let tx = app.callback(|state: &mut State, status: UpdateStatus| {
                    state.update_status = status;
                });
                app.spawn(async move {
                    let updater = AutoUpdater::new();
                    updater
                        .check_and_install_updates_with_callback(Some(
                            move |new_status: UpdateStatus| {
                                let tx = tx.clone();
                                async move {
                                    tx.send(new_status);
                                }
                            },
                        ))
                        .await;
                });
            }
        })
        .build(app.ctx())
        .height(25.)
}

fn channel_slider<'a>(
    key: u64,
    i: usize,
    binding: ([SliderState; 3], Binding<State, [SliderState; 3]>),
    values: [f32; 3],
    knob_color: Color,
    app: &mut AppState,
) -> Layout<'a, View<State>, AppCtx> {
    let ch = &CHANNELS[i];
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
        id!(key),
        (
            binding.0[i],
            Binding::new(
                move |s: &State| s.sliders[i],
                move |s: &mut State, v| s.sliders[i] = v,
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
    .on_change(move |state, _, val| {
        state.values[i] = val;
        state.update_ui();
    })
    .build(app.ctx())
    .height(26.)
    .pad_y(2.)
}
