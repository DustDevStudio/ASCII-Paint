use crate::algo::{FloodFill, PaintChannel, ShapeRasterizer};
use crate::history::{AddFrameCommand, History};
use crate::model::{
    CanvasSpec, Cell, CellCoord, CellRect, DomainError, Fps, Grid, Project, Rgb,
};
use crate::selection::{Selection, SelectionMode};
use crate::tools::{
    Brush, EditCellTool, Modifiers, Tool, ToolContext, ToolEvent,
};

#[test]
fn test_rgb_and_hex() {
    let black = Rgb::BLACK;
    assert_eq!(black.to_hex(), "#000000");
    assert_eq!(Rgb::WHITE.to_hex(), "#FFFFFF");

    let parsed = Rgb::from_hex("#123456").unwrap();
    assert_eq!(parsed, Rgb::new(0x12, 0x34, 0x56));
    assert_eq!(parsed.to_rgba(255), [0x12, 0x34, 0x56, 255]);

    assert!(Rgb::from_hex("invalid").is_err());
    assert!(Rgb::from_hex("#12345").is_err());
}

#[test]
fn test_cell_rect_and_canvas_spec() {
    let a = CellCoord::new(2, 5);
    let b = CellCoord::new(7, 3);
    let rect = CellRect::from_corners(a, b);
    assert_eq!(rect.min, CellCoord::new(2, 3));
    assert_eq!(rect.max, CellCoord::new(7, 5));
    assert_eq!(rect.width(), 6);
    assert_eq!(rect.height(), 3);
    assert!(rect.contains(CellCoord::new(4, 4)));
    assert!(!rect.contains(CellCoord::new(1, 4)));

    let spec = CanvasSpec::default();
    assert_eq!(spec.cols, 240);
    assert_eq!(spec.rows, 72);
    assert_eq!(spec.pixel_width(), 1920);
    assert_eq!(spec.pixel_height(), 1080);
    assert_eq!(spec.clamp(-5, 100), CellCoord::new(0, 71));
}

#[test]
fn test_grid_and_frame() {
    let spec = CanvasSpec {
        cols: 10,
        rows: 5,
        cell_w: 8,
        cell_h: 15,
    };
    let mut grid = Grid::blank(&spec);
    assert_eq!(grid.cols(), 10);
    assert_eq!(grid.rows(), 5);
    assert!(grid.get(CellCoord::new(0, 0)).unwrap().is_blank());

    let test_cell = Cell {
        glyph: 'X',
        fg: Rgb::new(255, 0, 0),
        bg: Some(Rgb::new(0, 255, 0)),
    };
    let prev = grid.set(CellCoord::new(2, 3), test_cell);
    assert_eq!(prev, Some(Cell::EMPTY));
    assert_eq!(grid.get(CellCoord::new(2, 3)), Some(&test_cell));
}

#[test]
fn test_project_and_timeline() {
    let now = 1234567890;
    let mut proj = Project::new("TestProj".to_string(), Rgb::BLACK, Fps(12), now);
    assert_eq!(proj.meta.name, "TestProj");
    assert_eq!(proj.timeline.len(), 1);
    assert_eq!(proj.timeline.selected_index(), 0);

    // Timeline remove last frame must fail
    assert_eq!(
        proj.timeline.remove(0).unwrap_err(),
        DomainError::CannotRemoveLastFrame
    );

    // Add frame via command
    let mut hist = History::new();
    let add_cmd = AddFrameCommand::blank_after(&mut proj, 0);
    hist.execute(Box::new(add_cmd), &mut proj);
    assert_eq!(proj.timeline.len(), 2);
    assert_eq!(proj.timeline.selected_index(), 1);
    assert!(hist.is_dirty());

    // Undo add frame
    hist.undo(&mut proj);
    assert_eq!(proj.timeline.len(), 1);
    assert_eq!(proj.timeline.selected_index(), 0);
    assert!(!hist.is_dirty());
}

#[test]
fn test_selection() {
    let spec = CanvasSpec {
        cols: 10,
        rows: 10,
        cell_w: 8,
        cell_h: 15,
    };
    let mut sel = Selection::new(&spec);
    assert!(sel.is_empty());
    assert!(sel.permits(CellCoord::new(0, 0))); // empty permits all

    let cells = vec![CellCoord::new(1, 1), CellCoord::new(1, 2)];
    sel.apply(&cells, SelectionMode::Replace);
    assert_eq!(sel.count(), 2);
    assert!(sel.contains(CellCoord::new(1, 1)));
    assert!(!sel.contains(CellCoord::new(0, 0)));
    assert!(sel.permits(CellCoord::new(1, 1)));
    assert!(!sel.permits(CellCoord::new(0, 0))); // non-empty restricts

    sel.apply(&[CellCoord::new(1, 1)], SelectionMode::Subtract);
    assert_eq!(sel.count(), 1);
    assert!(!sel.contains(CellCoord::new(1, 1)));
    assert!(sel.contains(CellCoord::new(1, 2)));

    sel.select_all();
    assert_eq!(sel.count(), 100);
    sel.clear();
    assert!(sel.is_empty());
}

#[test]
fn test_algorithms_rasterizer() {
    let a = CellCoord::new(0, 0);
    let b = CellCoord::new(3, 0);
    let line = ShapeRasterizer::line(a, b);
    assert_eq!(
        line,
        vec![
            CellCoord::new(0, 0),
            CellCoord::new(1, 0),
            CellCoord::new(2, 0),
            CellCoord::new(3, 0),
        ]
    );

    let spec = CanvasSpec {
        cols: 10,
        rows: 10,
        cell_w: 8,
        cell_h: 15,
    };
    let rect_cells = ShapeRasterizer::rect(CellCoord::new(1, 1), CellCoord::new(2, 2), &spec);
    assert_eq!(rect_cells.len(), 4);

    let ellipse_cells = ShapeRasterizer::ellipse(CellCoord::new(1, 1), CellCoord::new(1, 1), &spec);
    assert_eq!(ellipse_cells, vec![CellCoord::new(1, 1)]);

    let poly = ShapeRasterizer::polygon(
        &[
            CellCoord::new(1, 1),
            CellCoord::new(4, 1),
            CellCoord::new(4, 4),
            CellCoord::new(1, 4),
        ],
        &spec,
    );
    assert!(!poly.is_empty());
}

#[test]
fn test_flood_fill() {
    let spec = CanvasSpec {
        cols: 5,
        rows: 5,
        cell_w: 8,
        cell_h: 15,
    };
    let grid = Grid::blank(&spec);
    let sel = Selection::new(&spec);

    // All cells initially have fg WHITE and bg None
    let region = FloodFill::region(&grid, CellCoord::new(0, 0), PaintChannel::Foreground, &sel);
    assert_eq!(region.len(), 25);
}

#[test]
fn test_edit_cell_typing_tool() {
    let mut proj = Project::new("TypingTest".to_string(), Rgb::BLACK, Fps(12), 0);
    let mut sel = Selection::new(&proj.canvas);
    let brush = Brush::default();
    let mut hist = History::new();

    let mut tool = EditCellTool::new();
    let mut ctx = ToolContext {
        project: &mut proj,
        selection: &mut sel,
        brush: &brush,
        history: &mut hist,
    };

    // Click at (0, 0)
    tool.on_event(
        ToolEvent::PointerDown(CellCoord::new(0, 0), Modifiers::default()),
        &mut ctx,
    );
    // Type 'H'
    tool.on_event(ToolEvent::Text('H'), &mut ctx);
    // Type 'i'
    tool.on_event(ToolEvent::Text('i'), &mut ctx);

    // Verify live update
    let frame = ctx.project.timeline.selected_frame();
    assert_eq!(frame.grid.get(CellCoord::new(0, 0)).unwrap().glyph, 'H');
    assert_eq!(frame.grid.get(CellCoord::new(1, 0)).unwrap().glyph, 'i');

    // Deactivate / commit
    tool.on_event(ToolEvent::Deactivate, &mut ctx);
    assert!(ctx.history.can_undo());

    // Undo
    ctx.history.undo(ctx.project);
    let frame_undone = ctx.project.timeline.selected_frame();
    assert_eq!(
        frame_undone.grid.get(CellCoord::new(0, 0)).unwrap().glyph,
        ' '
    );
    assert_eq!(
        frame_undone.grid.get(CellCoord::new(1, 0)).unwrap().glyph,
        ' '
    );
}
