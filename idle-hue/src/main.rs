#![windows_subsystem = "windows"]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

use arboard::Clipboard;
use color::{AlphaColor, Oklch, Srgb};
use std::rc::Rc;
use ui::*;

const BG: Color = Color::from_rgb8(0x1e, 0x1e, 0x1e);
const FIELD_BG: Color = Color::from_rgb8(0x2d, 0x2d, 0x2d);
const FIELD_BORDER: Color = Color::from_rgb8(0x50, 0x50, 0x50);
const LABEL_COLOR: Color = Color::from_rgb8(0xaa, 0xaa, 0xaa);

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

struct State {
    values: [f32; 3],
    sliders: [SliderState; 3],
    fields: [TextState; 3],
    copy_buttons: [ButtonState; 3],
}

impl State {
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
        };
        s.update_ui();
        s
    }
}

fn main() {
    App::builder(State::default(), view)
        .title("idle-hue")
        .inner_size(400, 300)
        .start()
}

fn view<'a>(s: &'a State, app: &mut AppState) -> Layout<'a, View<State>, AppCtx> {
    stack(vec![
        rect(id!()).fill(BG).corner_rounding(0.).build(app.ctx()),
        column_spaced(
            12.,
            vec![
                space().height(10.),
                row_spaced(
                    10.,
                    vec![
                        rect(id!())
                            .fill(s.display_color())
                            .stroke(FIELD_BORDER, Stroke::new(1.))
                            .corner_rounding(8.)
                            .build(app.ctx()),
                        column_spaced(
                            10.,
                            (0..3)
                                .map(|i| {
                                    let formatted = s.formats()[i].clone();
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
                                            .fill(BrushSource::Dynamic(Rc::new(
                                                move |_, _: &()| {
                                                    let c = match (btn.depressed, btn.hovered) {
                                                        (true, _) => {
                                                            FIELD_BG.map_lightness(|l| l - 0.05)
                                                        }
                                                        (false, true) => {
                                                            FIELD_BG.map_lightness(|l| l + 0.05)
                                                        }
                                                        (false, false) => FIELD_BG,
                                                    };
                                                    Brush::Solid(c)
                                                },
                                            )))
                                            .stroke(FIELD_BORDER, Stroke::new(1.))
                                            .corner_rounding(6.)
                                            .build(ctx)
                                    })
                                    .label(move |_, ctx| {
                                        row_spaced(
                                            6.,
                                            vec![
                                                text(id!(i as u64), &formatted)
                                                    .font_size(12)
                                                    .fill(LABEL_COLOR)
                                                    .build(ctx),
                                                svg(id!(i as u64), COPY_ICON)
                                                    .fill(Brush::Solid(LABEL_COLOR))
                                                    .finish(ctx)
                                                    .width(14.)
                                                    .height(14.),
                                            ],
                                        )
                                        .pad(5.)
                                    })
                                    .on_click(move |state, _| {
                                        let text = state.formats()[i].clone();
                                        if let Ok(mut cb) = Clipboard::new() {
                                            let _ = cb.set_text(text);
                                        }
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
                                            .font_size(14)
                                            .font_weight(FontWeight::BOLD)
                                            .fill(LABEL_COLOR)
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
                                    channel_field(id!(i as u64), i, binding!(s, State, fields), app)
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
    .on_change(move |state, _, val| {
        state.values[i] = val;
        state.update_ui();
    })
    .build(app.ctx())
    .height(30.)
}

fn channel_field<'a>(
    key: u64,
    i: usize,
    binding: ([TextState; 3], Binding<State, [TextState; 3]>),
    app: &mut AppState,
) -> Layout<'a, View<State>, AppCtx> {
    let ch = &CHANNELS[i];
    let min = ch.min;
    let max = ch.max;
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
    .font_size(13)
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
            .fill(FIELD_BG)
            .stroke(FIELD_BORDER, Stroke::new(1.))
            .corner_rounding(4.)
            .build(ctx)
    })
    .build(app.ctx())
    .height(30.)
    .expand_x()
}
