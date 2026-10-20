use crate::brush::{Brush, BrushError, BrushId, BrushLibrary, Stamp};
use crate::compose::Compositor;
use crate::model::{
    CanvasSpec, CellCoord, Fps, Frame, FrameId, ImportFit, InkCell, Layer, Project, Rgb, Rgba,
};
use crate::selection::{Selection, SelectionMode};
use crate::tools::{
    Modifiers, PaintOperation, PaintTool, PickTool, Tool, ToolContext, ToolEvent, ToolId,
    ToolOptions, ToolResponse,
};
use crate::history::History;

#[test]
fn test_rgba_over_and_blending() {
    let red_half = Rgba::new(255, 0, 0, 128);
    let blue_opaque = Rgba::new(0, 0, 255, 255);
    let blended1 = red_half.over(blue_opaque);
    assert_eq!(blended1, Rgba::new(128, 0, 127, 255));

    let green_half = Rgba::new(0, 255, 0, 128);
    let blended2 = red_half.over(green_half);
    assert_eq!(blended2, Rgba::new(170, 85, 0, 192));

    let arbitrary = Rgba::new(10, 20, 30, 200);
    assert_eq!(arbitrary.over(Rgba::TRANSPARENT), arbitrary);
}

#[test]
fn test_canvas_spec_and_fit_content() {
    let spec = CanvasSpec {
        cols: 240,
        rows: 72,
        cell_w: 8,
        cell_h: 15,
    };
    assert_eq!(spec.fit_content(100, 50), ImportFit::Fits);

    match spec.fit_content(300, 90) {
        ImportFit::TooBig {
            suggested,
            cut_if_kept,
            cut_if_resized,
        } => {
            assert_eq!(suggested.cols, 300);
            assert_eq!(suggested.rows, 90);
            assert_eq!(cut_if_kept, (60, 18));
            assert_eq!(cut_if_resized, (0, 0));
        }
        _ => panic!("Expected TooBig"),
    }
}

#[test]
fn test_layers_and_compositing() {
    let spec = CanvasSpec {
        cols: 2,
        rows: 1,
        cell_w: 8,
        cell_h: 15,
    };
    let mut frame = Frame::blank(FrameId(1), &spec, Rgb::new(0x10, 0x10, 0x20));

    // Base layer (layer 0)
    let base_id = frame.active_layer().id;
    frame.set_ink(
        base_id,
        CellCoord::new(0, 0),
        InkCell::new('H', Rgba::WHITE),
    );
    frame.set_ink(
        base_id,
        CellCoord::new(1, 0),
        InkCell::new('i', Rgba::new(255, 0, 0, 255)),
    );
    frame.set_fill(
        base_id,
        CellCoord::new(1, 0),
        Rgba::new(0, 0, 255, 128),
    );

    // Top layer (layer 1) - hidden
    let top_id = frame.alloc_layer_id();
    let mut top_layer = Layer::blank(top_id, "top".to_string(), &spec);
    top_layer.visible = false;
    top_layer.opacity = 128;
    top_layer.ink.set(
        CellCoord::new(1, 0),
        InkCell::new('X', Rgba::new(0, 255, 0, 255)),
    );
    frame.insert_layer(1, top_layer).unwrap();

    let flat = Compositor::flatten(&frame, &spec);

    // Hidden layer should not show
    let c0 = flat.grid.get(CellCoord::new(0, 0)).unwrap();
    assert_eq!(c0.glyph, 'H');
    assert_eq!(c0.ink, Rgba::WHITE);

    let c1 = flat.grid.get(CellCoord::new(1, 0)).unwrap();
    assert_eq!(c1.glyph, 'i');
    assert_eq!(c1.ink, Rgba::new(255, 0, 0, 255));
    assert_eq!(c1.fill, Rgba::new(0, 0, 255, 128));
}

#[test]
fn test_brush_library_and_validation() {
    let mut lib = BrushLibrary::with_builtins();
    assert_eq!(BrushLibrary::default_id(), BrushId(7));
    let hash_brush = lib.get(BrushId(7)).unwrap();
    assert_eq!(hash_brush.fill_glyph(), '#');

    // Create valid custom brush
    let custom = Brush {
        id: BrushId(0),
        name: "Box".to_string(),
        stamp: Stamp::new(
            2,
            2,
            vec![Some('+'), Some('-'), Some('|'), Some('+')],
            (0, 0),
        ),
    };
    let new_id = lib.upsert(custom).unwrap();
    assert!(new_id.0 >= 1000);

    // Builtin cannot be removed
    assert_eq!(lib.remove(BrushId(1)), Err(BrushError::BuiltinReadOnly));
    // Custom can be removed
    assert!(lib.remove(new_id).is_ok());
}

#[test]
fn test_selection_invert_translate() {
    let spec = CanvasSpec {
        cols: 10,
        rows: 10,
        cell_w: 8,
        cell_h: 15,
    };
    let mut sel = Selection::new(&spec);
    sel.apply(&[CellCoord::new(1, 1), CellCoord::new(1, 2)], SelectionMode::Replace);
    assert_eq!(sel.count(), 2);

    assert!(sel.translate((1, 1)));
    assert!(!sel.contains(CellCoord::new(1, 1)));
    assert!(sel.contains(CellCoord::new(2, 2)));
    assert!(sel.contains(CellCoord::new(2, 3)));

    sel.invert();
    assert_eq!(sel.count(), 98);
    assert!(!sel.contains(CellCoord::new(2, 2)));
    assert!(sel.contains(CellCoord::new(0, 0)));
}

#[test]
fn test_paint_and_pick_tools() {
    let mut proj = Project::new("ToolTest".to_string(), Rgb::BLACK, Fps(12), 0);
    let mut sel = Selection::new(&proj.canvas);
    let mut hist = History::new();
    let options = ToolOptions::default();
    let brushes = BrushLibrary::with_builtins();

    let mut paint = PaintTool::new(ToolId::Paint, PaintOperation);
    let mut ctx = ToolContext {
        project: &mut proj,
        history: &mut hist,
        selection: &mut sel,
        options: &options,
        brushes: &brushes,
        mods: Modifiers::default(),
    };

    // Paint at (5, 5)
    paint.on_event(ToolEvent::PointerDown(Some(CellCoord::new(5, 5))), &mut ctx);
    paint.on_event(ToolEvent::PointerUp(Some(CellCoord::new(5, 5))), &mut ctx);

    let frame = ctx.project.timeline.selected_frame();
    let layer = frame.active_layer();
    assert_eq!(layer.ink.get(CellCoord::new(5, 5)).unwrap().glyph, '#');

    // Pick at (5, 5)
    let mut pick = PickTool::new();
    let resp = pick.on_event(ToolEvent::PointerDown(Some(CellCoord::new(5, 5))), &mut ctx);
    match resp {
        ToolResponse::Picked(colors) => {
            assert_eq!(colors.ink, Some(Rgba::WHITE));
        }
        _ => panic!("Expected Picked response"),
    }
}
