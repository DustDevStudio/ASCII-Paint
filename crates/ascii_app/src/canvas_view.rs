use std::sync::Arc;
use ascii_core::model::CellCoord;
use ascii_core::tools::{Modifiers, ToolContext, ToolEvent, ToolKey};
use ascii_render::GlyphSource;

use crate::renderer::CanvasRenderer;

pub struct CanvasView {
    pub renderer: CanvasRenderer,
    pub hover: Option<CellCoord>,
    space_pan: bool,
}

impl CanvasView {
    pub fn new(ctx: &egui::Context, glyphs: Arc<dyn GlyphSource>) -> Self {
        Self {
            renderer: CanvasRenderer::new(ctx, glyphs),
            hover: None,
            space_pan: false,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        session: &mut crate::editor::EditorSession,
    ) -> Option<CellCoord> {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

        if session.viewport.needs_fit {
            session.viewport.fit(rect, &session.project.canvas);
        }

        let is_playing = session.playback.is_playing();

        // Check Space key
        self.space_pan = ui.input(|i| i.key_down(egui::Key::Space));

        // 1. Mouse wheel zoom
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta != 0.0 && response.hovered() {
            if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
                let notches = scroll_delta / 50.0;
                session.viewport.zoom_at(mouse_pos, notches, rect);
            }
        }

        // 2. Pan: Middle drag or Space + Left drag
        let drag_delta = response.drag_delta();
        let is_middle_drag = ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
        let is_space_drag = self.space_pan && ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary));

        if (is_middle_drag || is_space_drag) && drag_delta != egui::Vec2::ZERO {
            session.viewport.pan_by(drag_delta, rect, &session.project.canvas);
        }

        // 3. Hovered cell
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
        self.hover = pointer_pos.and_then(|p| {
            session
                .viewport
                .screen_to_cell(p, &session.project.canvas, rect)
        });

        // 4. Tools and Keyboard (only if not playing and not space-panning)
        if !is_playing && !self.space_pan {
            let mods = ui.input(|i| Modifiers {
                shift: i.modifiers.shift,
                alt: i.modifiers.alt,
                ctrl: i.modifiers.ctrl || i.modifiers.command,
            });

            // Pointer events
            if response.clicked() || response.drag_started() {
                ui.memory_mut(|m| m.request_focus(response.id));
                if let Some(pos) = pointer_pos {
                    if let Some(c) = session
                        .viewport
                        .screen_to_cell(pos, &session.project.canvas, rect)
                    {
                        let mut ctx = ToolContext {
                            project: &mut session.project,
                            selection: &mut session.selection,
                            brush: &session.brush,
                            history: &mut session.history,
                        };
                        session.tool_registry.dispatch(ToolEvent::PointerDown(c, mods), &mut ctx);
                    }
                }
            } else if response.dragged() {
                let cell = pointer_pos.and_then(|p| {
                    session
                        .viewport
                        .screen_to_cell(p, &session.project.canvas, rect)
                });
                let mut ctx = ToolContext {
                    project: &mut session.project,
                    selection: &mut session.selection,
                    brush: &session.brush,
                    history: &mut session.history,
                };
                session
                    .tool_registry
                    .dispatch(ToolEvent::PointerMove(cell, mods), &mut ctx);
            } else if response.drag_stopped() {
                let cell = pointer_pos.and_then(|p| {
                    session
                        .viewport
                        .screen_to_cell(p, &session.project.canvas, rect)
                });
                let mut ctx = ToolContext {
                    project: &mut session.project,
                    selection: &mut session.selection,
                    brush: &session.brush,
                    history: &mut session.history,
                };
                session
                    .tool_registry
                    .dispatch(ToolEvent::PointerUp(cell, mods), &mut ctx);
            }

            // Keyboard input (when focused)
            if ui.memory(|m| m.has_focus(response.id)) {
                let events = ui.input(|i| i.events.clone());
                for ev in events {
                    match ev {
                        egui::Event::Text(text) => {
                            for ch in text.chars() {
                                if !ch.is_control() {
                                    let mut ctx = ToolContext {
                                        project: &mut session.project,
                                        selection: &mut session.selection,
                                        brush: &session.brush,
                                        history: &mut session.history,
                                    };
                                    session.tool_registry.dispatch(ToolEvent::Text(ch), &mut ctx);
                                }
                            }
                        }
                        egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } => {
                            let tool_mods = Modifiers {
                                shift: modifiers.shift,
                                alt: modifiers.alt,
                                ctrl: modifiers.ctrl || modifiers.command,
                            };
                            let tool_key = match key {
                                egui::Key::ArrowUp => Some(ToolKey::ArrowUp),
                                egui::Key::ArrowDown => Some(ToolKey::ArrowDown),
                                egui::Key::ArrowLeft => Some(ToolKey::ArrowLeft),
                                egui::Key::ArrowRight => Some(ToolKey::ArrowRight),
                                egui::Key::Enter => Some(ToolKey::Enter),
                                egui::Key::Backspace => Some(ToolKey::Backspace),
                                egui::Key::Delete => Some(ToolKey::Delete),
                                egui::Key::Escape => Some(ToolKey::Escape),
                                _ => None,
                            };
                            if let Some(tk) = tool_key {
                                let mut ctx = ToolContext {
                                    project: &mut session.project,
                                    selection: &mut session.selection,
                                    brush: &session.brush,
                                    history: &mut session.history,
                                };
                                session
                                    .tool_registry
                                    .dispatch(ToolEvent::Key(tk, tool_mods), &mut ctx);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Paint
        let painter = ui.painter_at(rect);
        let time = ui.input(|i| i.time);
        let overlay = session.tool_registry.active().overlay();

        self.renderer.paint(
            &painter,
            rect,
            &session.project.canvas,
            session.project.timeline.selected_frame(),
            &session.selection,
            &session.viewport,
            &overlay,
            self.hover,
            session.show_grid,
            is_playing,
            time,
        );

        self.hover
    }
}
