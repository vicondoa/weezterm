use crate::customglyph::*;
use crate::tabbar::{TabBarItem, TabEntry};
use crate::termwindow::box_model::*;

use crate::termwindow::render::window_buttons::window_button_element;
use crate::termwindow::{UIItem, UIItemType};
use crate::utilsprites::RenderMetrics;
use config::{Dimension, DimensionContext, RgbaColor, TabBarColor, TabBarColors};
use std::rc::Rc;
use wezterm_font::LoadedFont;
use wezterm_term::color::{ColorAttribute, ColorPalette, SrgbaTuple};
use window::{IntegratedTitleButtonAlignment, IntegratedTitleButtonStyle};

// --- weezterm remote features ---
/// Derive sensible tab bar colors from a terminal color palette.
/// This is used when no explicit `tab_bar` colors are configured, so
/// the tab bar follows the active color scheme automatically.
fn derive_tab_bar_colors_from_palette(palette: &ColorPalette) -> TabBarColors {
    let bg = palette.background;
    let fg = palette.foreground;

    // Helper to convert SrgbaTuple to RgbaColor
    let to_rgba = |c: SrgbaTuple| -> RgbaColor {
        let (r, g, b, _a) = c.as_rgba_u8();
        RgbaColor::from((r, g, b))
    };

    // Helper to lighten or darken a color for visual hierarchy
    let adjust = |c: SrgbaTuple, amount: i16| -> RgbaColor {
        let (r, g, b, _a) = c.as_rgba_u8();
        let r = (r as i16 + amount).clamp(0, 255) as u8;
        let g = (g as i16 + amount).clamp(0, 255) as u8;
        let b = (b as i16 + amount).clamp(0, 255) as u8;
        RgbaColor::from((r, g, b))
    };

    // Detect if this is a "light" scheme (bright background)
    let (br, bg_g, bb, _) = bg.as_rgba_u8();
    let luminance = (br as u16 + bg_g as u16 + bb as u16) / 3;
    let is_light = luminance > 128;

    // For light themes, darken for contrast; for dark themes, lighten
    let step = if is_light { -25i16 } else { 25i16 };

    // Active tab should visually pop against the bar background.
    // Always invert: use fg as bg and bg as fg so the active tab
    // stands out regardless of whether the scheme is light or dark.
    let (active_bg, active_fg) = (to_rgba(fg), to_rgba(bg));

    TabBarColors {
        background: Some(adjust(bg, step)),
        active_tab: Some(TabBarColor {
            bg_color: active_bg,
            fg_color: active_fg,
            ..TabBarColor::default()
        }),
        inactive_tab: Some(TabBarColor {
            bg_color: adjust(bg, step),
            fg_color: adjust(fg, if is_light { 80 } else { -80 }),
            ..TabBarColor::default()
        }),
        inactive_tab_hover: Some(TabBarColor {
            bg_color: adjust(bg, step * 2),
            fg_color: to_rgba(fg),
            ..TabBarColor::default()
        }),
        new_tab: Some(TabBarColor {
            bg_color: adjust(bg, step),
            fg_color: adjust(fg, if is_light { 80 } else { -80 }),
            ..TabBarColor::default()
        }),
        new_tab_hover: Some(TabBarColor {
            bg_color: adjust(bg, step * 2),
            fg_color: to_rgba(fg),
            ..TabBarColor::default()
        }),
        inactive_tab_edge: Some(to_rgba(fg)),
        inactive_tab_edge_hover: Some(to_rgba(fg)),
    }
}
// --- end weezterm remote features ---

const X_BUTTON: &[Poly] = &[
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
            PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
            PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
];

const PLUS_BUTTON: &[Poly] = &[
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
            PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
            PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
];

// --- weezterm remote features ---
/// Down-pointing chevron (▾) for the new-tab dropdown button.
const CHEVRON_DOWN: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Frac(1, 6), BlockCoord::Frac(1, 4)),
        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 4)),
        PolyCommand::LineTo(BlockCoord::Frac(5, 6), BlockCoord::Frac(1, 4)),
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Outline,
}];
// --- end weezterm remote features ---

impl crate::TermWindow {
    pub fn invalidate_fancy_tab_bar(&mut self) {
        self.fancy_tab_bar.take();
    }

    pub fn build_fancy_tab_bar(&self, palette: &ColorPalette) -> anyhow::Result<ComputedElement> {
        let _t = std::time::Instant::now();
        let tab_bar_height = self.tab_bar_pixel_height()?;
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let items = self.tab_bar.items();
        // --- weezterm remote features ---
        // Derive tab bar colors from the resolved palette (which includes
        // the active color scheme), falling back to the explicit `colors.tab_bar`
        // config, then to scheme-derived defaults.  This ensures the tab bar
        // updates when the user changes `color_scheme`.
        let colors = self
            .config
            .resolved_palette
            .tab_bar
            .clone()
            .or_else(|| {
                self.config
                    .colors
                    .as_ref()
                    .and_then(|c| c.tab_bar.clone())
            })
            .unwrap_or_else(|| derive_tab_bar_colors_from_palette(palette));
        // --- end weezterm remote features ---

        let mut left_status = vec![];
        let mut left_eles = vec![];
        let mut right_eles = vec![];
        // --- weezterm remote features ---
        // Derive titlebar background/foreground from palette when window_frame
        // uses its hardcoded defaults, so the tab bar matches the color scheme.
        let frame_bg = self.config.resolved_palette.background
            .map(|c| {
                let (r, g, b, _a) = c.to_srgb_u8();
                // Slightly darken bg for bar distinction
                config::RgbaColor::from((
                    r.saturating_sub(10),
                    g.saturating_sub(10),
                    b.saturating_sub(10),
                ))
            })
            .unwrap_or(if self.focused.is_some() {
                self.config.window_frame.active_titlebar_bg
            } else {
                self.config.window_frame.inactive_titlebar_bg
            });
        let frame_fg = self.config.resolved_palette.foreground
            .map(|c| {
                let (r, g, b, _a) = c.to_srgb_u8();
                config::RgbaColor::from((r, g, b))
            })
            .unwrap_or(if self.focused.is_some() {
                self.config.window_frame.active_titlebar_fg
            } else {
                self.config.window_frame.inactive_titlebar_fg
            });
        let bar_colors = ElementColors {
            border: BorderColor::default(),
            bg: frame_bg.to_linear().into(),
            text: frame_fg.to_linear().into(),
        };
        // --- end weezterm remote features ---

        let item_to_elem = |item: &TabEntry| -> Element {
            let element = Element::with_line(&font, &item.title, palette);

            let bg_color = item
                .title
                .get_cell(0)
                .and_then(|c| match c.attrs().background() {
                    ColorAttribute::Default => None,
                    col => Some(palette.resolve_bg(col)),
                });
            let fg_color = item
                .title
                .get_cell(0)
                .and_then(|c| match c.attrs().foreground() {
                    ColorAttribute::Default => None,
                    col => Some(palette.resolve_fg(col)),
                });

            let new_tab = colors.new_tab();
            let new_tab_hover = colors.new_tab_hover();
            let active_tab = colors.active_tab();

            match item.item {
                TabBarItem::RightStatus | TabBarItem::LeftStatus | TabBarItem::None => element
                    .item_type(UIItemType::TabBar(TabBarItem::None))
                    .line_height(Some(1.75))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.0),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.),
                        bottom: Dimension::Cells(0.),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(0.)))
                    .colors(bar_colors.clone()),
                TabBarItem::NewTabButton => Element::new(
                    &font,
                    ElementContent::Poly {
                        line_width: metrics.underline_height.max(2),
                        poly: SizedPoly {
                            poly: PLUS_BUTTON,
                            // --- weezterm remote features ---
                            width: Dimension::Pixels(metrics.cell_size.height as f32 * 0.6),
                            height: Dimension::Pixels(metrics.cell_size.height as f32 * 0.6),
                            // --- end weezterm remote features ---
                        },
                    },
                )
                .vertical_align(VerticalAlign::Middle)
                .item_type(UIItemType::TabBar(item.item.clone()))
                .margin(BoxDimension {
                    left: Dimension::Cells(0.5),
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.2),
                    bottom: Dimension::Cells(0.),
                })
                .padding(BoxDimension {
                    left: Dimension::Cells(0.5),
                    right: Dimension::Cells(0.5),
                    top: Dimension::Cells(0.2),
                    bottom: Dimension::Cells(0.25),
                })
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .colors(ElementColors {
                    border: BorderColor::default(),
                    bg: new_tab.bg_color.to_linear().into(),
                    text: new_tab.fg_color.to_linear().into(),
                })
                .hover_colors(Some(ElementColors {
                    border: BorderColor::default(),
                    bg: new_tab_hover.bg_color.to_linear().into(),
                    text: new_tab_hover.fg_color.to_linear().into(),
                })),
                // --- weezterm remote features ---
                TabBarItem::NewTabDropdown => Element::new(
                    &font,
                    ElementContent::Poly {
                        line_width: metrics.underline_height.max(2),
                        poly: SizedPoly {
                            poly: CHEVRON_DOWN,
                            width: Dimension::Pixels(metrics.cell_size.height as f32 * 0.4),
                            height: Dimension::Pixels(metrics.cell_size.height as f32 * 0.4),
                        },
                    },
                )
                .vertical_align(VerticalAlign::Middle)
                .item_type(UIItemType::TabBar(TabBarItem::NewTabDropdown))
                .margin(BoxDimension {
                    left: Dimension::Cells(0.),
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.2),
                    bottom: Dimension::Cells(0.),
                })
                .padding(BoxDimension {
                    left: Dimension::Cells(0.25),
                    right: Dimension::Cells(0.25),
                    top: Dimension::Cells(0.2),
                    bottom: Dimension::Cells(0.25),
                })
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .colors(ElementColors {
                    border: BorderColor::default(),
                    bg: new_tab.bg_color.to_linear().into(),
                    text: new_tab.fg_color.to_linear().into(),
                })
                .hover_colors(Some(ElementColors {
                    border: BorderColor::default(),
                    bg: new_tab_hover.bg_color.to_linear().into(),
                    text: new_tab_hover.fg_color.to_linear().into(),
                })),
                // --- end weezterm remote features ---
                TabBarItem::Tab { active, .. } if active => element
                    .vertical_align(VerticalAlign::Bottom)
                    .item_type(UIItemType::TabBar(item.item.clone()))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.2),
                        right: Dimension::Cells(0.2),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.25),
                    })
                    // --- weezterm remote features ---
                    // Clean rectangular border (no rounded corners — they
                    // cause rendering notches on high-contrast/eink displays)
                    .border(BoxDimension {
                        left: Dimension::Pixels(1.),
                        right: Dimension::Pixels(1.),
                        top: Dimension::Pixels(2.),
                        bottom: Dimension::Pixels(0.),
                    })
                    .colors(ElementColors {
                        border: BorderColor::new(
                            active_tab.bg_color.to_linear(),
                        ),
                        bg: active_tab.bg_color.to_linear().into(),
                        text: active_tab.fg_color.to_linear().into(),
                    }),
                    // --- end weezterm remote features ---
                TabBarItem::Tab { .. } => element
                    .vertical_align(VerticalAlign::Bottom)
                    .item_type(UIItemType::TabBar(item.item.clone()))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.2),
                        right: Dimension::Cells(0.2),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.),
                    })
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.2),
                        bottom: Dimension::Cells(0.25),
                    })
                    // --- weezterm remote features ---
                    // Clean rectangular border — no rounded corners
                    .border(BoxDimension {
                        left: Dimension::Pixels(1.),
                        right: Dimension::Pixels(1.),
                        top: Dimension::Pixels(1.),
                        bottom: Dimension::Pixels(0.),
                    })
                    .colors({
                        let inactive_tab = colors.inactive_tab();
                        ElementColors {
                            border: BorderColor::new(
                                inactive_tab.fg_color.to_linear(),
                            ),
                            bg: inactive_tab.bg_color.to_linear().into(),
                            text: inactive_tab.fg_color.to_linear().into(),
                        }
                    })
                    .hover_colors({
                        let inactive_tab_hover = colors.inactive_tab_hover();
                        Some(ElementColors {
                            border: BorderColor::new(
                                inactive_tab_hover.fg_color.to_linear(),
                            ),
                            bg: inactive_tab_hover.bg_color.to_linear().into(),
                            text: inactive_tab_hover.fg_color.to_linear().into(),
                        })
                    }),
                    // --- end weezterm remote features ---
                TabBarItem::WindowButton(button) => window_button_element(
                    button,
                    self.window_state.contains(window::WindowState::MAXIMIZED),
                    &font,
                    &metrics,
                    &self.config,
                ),
            }
        };

        let num_tabs: f32 = items
            .iter()
            .map(|item| match item.item {
                TabBarItem::NewTabButton | TabBarItem::Tab { .. } => 1.,
                _ => 0.,
            })
            .sum();
        let max_tab_width = ((self.dimensions.pixel_width as f32 / num_tabs)
            - (1.5 * metrics.cell_size.width as f32))
            .max(0.);

        // Reserve space for the native titlebar buttons
        if self
            .config
            .window_decorations
            .contains(::window::WindowDecorations::INTEGRATED_BUTTONS)
            && self.config.integrated_title_button_style == IntegratedTitleButtonStyle::MacOsNative
            && !self.window_state.contains(window::WindowState::FULL_SCREEN)
        {
            left_status.push(
                Element::new(&font, ElementContent::Text("".to_string())).margin(BoxDimension {
                    left: Dimension::Cells(4.0), // FIXME: determine exact width of macos ... buttons
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.),
                    bottom: Dimension::Cells(0.),
                }),
            );
        }

        for item in items {
            match item.item {
                TabBarItem::LeftStatus => left_status.push(item_to_elem(item)),
                TabBarItem::None | TabBarItem::RightStatus => right_eles.push(item_to_elem(item)),
                TabBarItem::WindowButton(_) => {
                    if self.config.integrated_title_button_alignment
                        == IntegratedTitleButtonAlignment::Left
                    {
                        left_eles.push(item_to_elem(item))
                    } else {
                        right_eles.push(item_to_elem(item))
                    }
                }
                TabBarItem::Tab { tab_idx, active } => {
                    let mut elem = item_to_elem(item);
                    elem.max_width = Some(Dimension::Pixels(max_tab_width));
                    elem.content = match elem.content {
                        ElementContent::Text(_) => unreachable!(),
                        ElementContent::Poly { .. } => unreachable!(),
                        ElementContent::Children(mut kids) => {
                            if self.config.show_close_tab_button_in_tabs {
                                kids.push(make_x_button(&font, &metrics, &colors, tab_idx, active));
                            }
                            ElementContent::Children(kids)
                        }
                    };
                    left_eles.push(elem);
                }
                _ => left_eles.push(item_to_elem(item)),
            }
        }

        let mut children = vec![];

        if !left_status.is_empty() {
            children.push(
                Element::new(&font, ElementContent::Children(left_status))
                    .colors(bar_colors.clone()),
            );
        }

        let window_buttons_at_left = self
            .config
            .window_decorations
            .contains(window::WindowDecorations::INTEGRATED_BUTTONS)
            && (self.config.integrated_title_button_alignment
                == IntegratedTitleButtonAlignment::Left
                || self.config.integrated_title_button_style
                    == IntegratedTitleButtonStyle::MacOsNative);

        let left_padding = if window_buttons_at_left {
            if self.config.integrated_title_button_style == IntegratedTitleButtonStyle::MacOsNative
            {
                if !self.window_state.contains(window::WindowState::FULL_SCREEN) {
                    Dimension::Pixels(70.0)
                } else {
                    Dimension::Cells(0.5)
                }
            } else {
                Dimension::Pixels(0.0)
            }
        } else {
            Dimension::Cells(0.5)
        };

        children.push(
            Element::new(&font, ElementContent::Children(left_eles))
                .vertical_align(VerticalAlign::Bottom)
                .colors(bar_colors.clone())
                .padding(BoxDimension {
                    left: left_padding,
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.),
                    bottom: Dimension::Cells(0.),
                })
                .zindex(1),
        );
        children.push(
            Element::new(&font, ElementContent::Children(right_eles))
                .colors(bar_colors.clone())
                .float(Float::Right),
        );

        let content = ElementContent::Children(children);

        let tabs = Element::new(&font, content)
            .display(DisplayType::Block)
            .item_type(UIItemType::TabBar(TabBarItem::None))
            .min_width(Some(Dimension::Pixels(self.dimensions.pixel_width as f32)))
            .min_height(Some(Dimension::Pixels(tab_bar_height)))
            .vertical_align(VerticalAlign::Bottom)
            .colors(bar_colors);

        let border = self.get_os_border();

        let mut computed = self.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: self.dimensions.pixel_height as f32,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: self.dimensions.pixel_width as f32,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect(
                    border.left.get() as f32,
                    0.,
                    self.dimensions.pixel_width as f32 - (border.left + border.right).get() as f32,
                    tab_bar_height,
                ),
                metrics: &metrics,
                gl_state: self.render_state.as_ref().unwrap(),
                zindex: 10,
            },
            &tabs,
        )?;

        computed.translate(euclid::vec2(
            0.,
            if self.config.tab_bar_at_bottom {
                self.dimensions.pixel_height as f32
                    - (computed.bounds.height() + border.bottom.get() as f32)
            } else {
                border.top.get() as f32
            },
        ));

        log::debug!("build_fancy_tab_bar completed in {:?}", _t.elapsed());
        Ok(computed)
    }

    pub fn paint_fancy_tab_bar(&self) -> anyhow::Result<Vec<UIItem>> {
        let computed = self.fancy_tab_bar.as_ref().ok_or_else(|| {
            anyhow::anyhow!("paint_fancy_tab_bar called but fancy_tab_bar is None")
        })?;
        let ui_items = computed.ui_items();

        let gl_state = self.render_state.as_ref().unwrap();
        self.render_element(&computed, gl_state, None)?;

        Ok(ui_items)
    }
}

fn make_x_button(
    font: &Rc<LoadedFont>,
    metrics: &RenderMetrics,
    colors: &TabBarColors,
    tab_idx: usize,
    active: bool,
) -> Element {
    Element::new(
        &font,
        ElementContent::Poly {
            line_width: metrics.underline_height.max(2),
            poly: SizedPoly {
                poly: X_BUTTON,
                width: Dimension::Pixels(metrics.cell_size.height as f32 / 2.),
                height: Dimension::Pixels(metrics.cell_size.height as f32 / 2.),
            },
        },
    )
    // Ensure that we draw our background over the
    // top of the rest of the tab contents
    .zindex(1)
    .vertical_align(VerticalAlign::Middle)
    .float(Float::Right)
    .item_type(UIItemType::CloseTab(tab_idx))
    .hover_colors({
        let inactive_tab_hover = colors.inactive_tab_hover();
        let active_tab = colors.active_tab();

        Some(ElementColors {
            border: BorderColor::default(),
            bg: (if active {
                inactive_tab_hover.bg_color
            } else {
                active_tab.bg_color
            })
            .to_linear()
            .into(),
            text: (if active {
                inactive_tab_hover.fg_color
            } else {
                active_tab.fg_color
            })
            .to_linear()
            .into(),
        })
    })
    .padding(BoxDimension {
        left: Dimension::Cells(0.25),
        right: Dimension::Cells(0.25),
        top: Dimension::Cells(0.25),
        bottom: Dimension::Cells(0.25),
    })
    .margin(BoxDimension {
        left: Dimension::Cells(0.5),
        right: Dimension::Cells(0.),
        top: Dimension::Cells(0.),
        bottom: Dimension::Cells(0.),
    })
}
