#![windows_subsystem = "windows"]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

use arboard::Clipboard;
use color::{AlphaColor, Oklch, Srgb};
use std::sync::Arc;
use tokio::sync::Mutex;
use ui::*;

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

struct State {
    values: [f32; 3],
    sliders: [SliderState; 3],
    fields: [TextState; 3],
    copy_buttons: [ButtonState; 3],
    dark_mode: bool,
    dark_mode_button: ButtonState,
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

    fn update_fields(&mut self) {
        self.fields[0] = TextState::new(format!("{:.2}", self.values[0]));
        self.fields[1] = TextState::new(format!("{:.3}", self.values[1]));
        self.fields[2] = TextState::new(format!("{:.1}", self.values[2]));
    }

    fn update_sliders(&mut self) {
        for i in 0..3 {
            self.sliders[i].value = self.values[i];
        }
    }

    fn update_ui(&mut self) {
        self.update_fields();
        self.update_sliders();
    }
}

impl Default for State {
    fn default() -> Self {
        let mut s = Self {
            values: [0.7, 0.15, 180.0],
            sliders: Default::default(),
            fields: Default::default(),
            copy_buttons: Default::default(),
            dark_mode: true,
            dark_mode_button: Default::default(),
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
            .inner_size(400, 300),
    )
    .start()
}

fn view<'a>(s: &'a State, app: &mut AppState) -> Layout<'a, View<State>, AppCtx> {
    let bg = s.theme(Theme::Gray0);
    let field_bg = s.theme(Theme::Gray30);
    let field_border = s.theme(Theme::Gray50);
    let label_color = s.theme_inverted(Theme::Gray0);

    stack(vec![
        rect(id!()).fill(bg).corner_rounding(0.).build(app.ctx()),
        column_spaced(
            12.,
            vec![
                row_spaced(
                    10.,
                    vec![
                        space().inert_y(),
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
                            .build(app.ctx()),
                        column_spaced(
                            10.,
                            (0..3)
                                .map(|i| {
                                    let copied = s.copied.try_lock().map(|c| c[i]).unwrap_or(false);
                                    let label = if copied {
                                        "copied".to_string()
                                    } else {
                                        match i {
                                            0 => s.format_hex().to_uppercase(),
                                            1 => "RGB".to_string(),
                                            _ => "OKLCH".to_string(),
                                        }
                                    };
                                    let copied_state = s.copied.clone();
                                    button(
                                        id!(i as u64),
                                        (
                                            s.copy_buttons[i],
                                            Binding::new(
                                                move |s: &State| s.copy_buttons[i],
                                                move |s: &mut State, v| s.copy_buttons[i] = v,
                                            ),
                                        ),
                                    )
                                    .surface(move |btn, ctx| {
                                        rect(id!(i as u64))
                                            .fill(btn_surface_color(btn, s.theme(Theme::Gray30)))
                                            .stroke(s.display_color(), Stroke::new(1.))
                                            .corner_rounding(6.)
                                            .build(ctx)
                                    })
                                    .label(move |btn, ctx| {
                                        let c = btn_label_color(btn, label_color);
                                        row_spaced(
                                            6.,
                                            vec![
                                                text(id!(i as u64), &label)
                                                    .font_size(16)
                                                    .fill(c)
                                                    .build(ctx),
                                                if copied {
                                                    svg(id!(i as u64), CHECKMARK_ICON)
                                                        .fill(Brush::Solid(c))
                                                        .finish(ctx)
                                                        .width(14.)
                                                        .height(14.)
                                                } else {
                                                    svg(id!(i as u64), COPY_ICON)
                                                        .fill(Brush::Solid(c))
                                                        .finish(ctx)
                                                        .width(14.)
                                                        .height(14.)
                                                },
                                            ],
                                        )
                                        .pad(5.)
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
                                            tokio::time::sleep(tokio::time::Duration::from_secs(2))
                                                .await;
                                            copied_reset.lock().await[i] = false;
                                            redraw.trigger().await;
                                        });
                                    })
                                    .build(app.ctx())
                                })
                                .collect(),
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
                                        s.theme(Theme::Gray30),
                                        s.theme_inverted(Theme::Gray0),
                                        s.theme(Theme::Gray50),
                                        s.theme(Theme::Gray70),
                                        app,
                                    )
                                })
                                .collect(),
                        )
                        .width_range(200.0..),
                        column_spaced(
                            8.,
                            (0..3)
                                .map(|i| {
                                    channel_field(
                                        id!(i as u64),
                                        i,
                                        binding!(s, State, fields),
                                        label_color,
                                        field_bg,
                                        field_border,
                                        app,
                                    )
                                })
                                .collect(),
                        ),
                    ],
                ),
            ],
        )
        .pad(20.),
    ])
}

fn channel_slider<'a>(
    key: u64,
    i: usize,
    binding: ([SliderState; 3], Binding<State, [SliderState; 3]>),
    background_color: Color,
    knob_color: Color,
    track_color: Color,
    traveled_track_color: Color,
    app: &mut AppState,
) -> Layout<'a, View<State>, AppCtx> {
    let ch = &CHANNELS[i];
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
            .fill(background_color)
            .corner_rounding(area.height)
            .build(ctx)
    })
    .track(move |_, area, ctx| {
        rect(id!(key))
            .fill(track_color)
            .corner_rounding(area.height)
            .build(ctx)
    })
    .traveled_track(move |_, area, ctx| {
        rect(id!(key))
            .fill(traveled_track_color)
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

fn channel_field<'a>(
    key: u64,
    i: usize,
    binding: ([TextState; 3], Binding<State, [TextState; 3]>),
    label_color: Color,
    field_bg: Color,
    field_border: Color,
    app: &mut AppState,
) -> Layout<'a, View<State>, AppCtx> {
    let ch = &CHANNELS[i];
    let min = ch.min;
    let max = ch.max;
    stack(vec![
        text_field(
            id!(key),
            (
                binding.0[i].clone(),
                Binding::new(
                    move |s: &State| s.fields[i].clone(),
                    move |s: &mut State, v| s.fields[i] = v,
                ),
            ),
        )
        .font_size(16)
        .text_fill(label_color)
        .enter_end_editing()
        .esc_end_editing()
        .on_edit(move |state, _, edit| match edit {
            EditInteraction::Update(t) => {
                if let Ok(v) = t.parse::<f32>() {
                    state.values[i] = v.clamp(min, max);
                    state.update_sliders();
                }
            }
            EditInteraction::End => state.update_ui(),
        })
        .background(move |_, _, ctx| {
            rect(id!(key))
                .fill(field_bg)
                .stroke(field_border, Stroke::new(1.))
                .corner_rounding(4.)
                .build(ctx)
        })
        .build(app.ctx())
        .height(26.)
        .expand_x(),
    ])
    .expand_x()
    .pad_y(2.)
}
