pub mod export;
pub mod format;
pub mod import;
pub mod storage;

pub use export::{
    AsciiSequenceWorker, Clock, ExportError, ExportJobHandle, ExportMessage, ExportService,
    FfmpegLocator, Mp4Worker, SequenceNamer,
};
pub use format::{
    AanimCodec, AsciiAnimation, AsciiCodec, AsciiImage, AsprCodec, ByteReader, ContainerCodec,
    ContainerHeader, FileKind, FlatFrameBlockCodec, FormatCodec, FormatError,
    LayeredFrameBlockCodec,
};
pub use import::{
    ImportError, ImportKind, ImportService, ImportSource, ImportedFile, ParsedText, TextImporter,
};
pub use storage::{
    AtomicWriter, BrushRecord, BrushStore, BrushStoreError, ProjectRepository, RecentEntry,
    RecentStore, RepoError,
};

#[cfg(test)]
mod tests {
    use super::*;
    use ascii_core::brush::{Brush, BrushId, Stamp};
    use ascii_core::compose::{FlatCell, FlatFrame};
    use ascii_core::model::{
        CanvasSpec, CellCoord, Fps, Frame, FrameId, Grid, InkCell, Layer, LayerId, Project, Rgb,
        Rgba,
    };

    #[test]
    fn test_test_vector_08c() {
        // Test vector from diagram 08c:
        // Image content (flattened): cell 0 = 'H' white (opaque) on nothing (fill transparent).
        // cell 1 = 'i' red (opaque) on blue at alpha 0x80 (50 %).
        // Frame background #000000. Canvas 2 x 1, cell 8 x 15, font_name "default".
        let spec = CanvasSpec {
            cols: 2,
            rows: 1,
            cell_w: 8,
            cell_h: 15,
        };
        let mut grid = Grid::filled(&spec, FlatCell::EMPTY);
        grid.set(
            CellCoord::new(0, 0),
            FlatCell {
                glyph: 'H',
                ink: Rgba::WHITE,
                fill: Rgba::TRANSPARENT,
            },
        );
        grid.set(
            CellCoord::new(1, 0),
            FlatCell {
                glyph: 'i',
                ink: Rgba::new(255, 0, 0, 255),
                fill: Rgba::new(0, 0, 255, 0x80),
            },
        );

        let flat_frame = FlatFrame {
            background: Rgb::BLACK,
            grid,
        };

        let image = AsciiImage {
            canvas: spec,
            font_name: "default".to_string(),
            frame: flat_frame,
        };

        let expected_bytes: [u8; 62] = [
            // ContainerHeader (20 bytes)
            0x41, 0x53, 0x43, 0x49, // "ASCI"
            0x01, 0x00,             // version = 1
            0x00, 0x00,             // flags = 0 (uncompressed)
            0x2A, 0x00, 0x00, 0x00, // uncompressed_len = 42
            0x2A, 0x00, 0x00, 0x00, // stored_len = 42
            0xFE, 0x21, 0x71, 0xA0, // crc32 = 0xA07121FE
            // CanvasHeader (15 bytes)
            0x02, 0x00,             // cols = 2
            0x01, 0x00,             // rows = 1
            0x08,                   // cell_w = 8
            0x0F,                   // cell_h = 15
            0x07, 0x00,             // font_name len = 7
            0x64, 0x65, 0x66, 0x61, 0x75, 0x6C, 0x74, // "default"
            // FlatFrameBlock (27 bytes)
            0x00, 0x00, 0x00,       // background = #000000
            0x48, 0x00, 0x00, 0x00, // glyph 0 = 'H'
            0x69, 0x00, 0x00, 0x00, // glyph 1 = 'i'
            0xFF, 0xFF, 0xFF, 0xFF, // ink 0 = white opaque
            0xFF, 0x00, 0x00, 0xFF, // ink 1 = red opaque
            0x00, 0x00, 0x00, 0x00, // fill 0 = nothing
            0x00, 0x00, 0xFF, 0x80, // fill 1 = blue alpha 0x80
        ];

        let codec = AsciiCodec;
        let encoded = codec.encode(&image).expect("encode succeeds");
        assert_eq!(encoded, expected_bytes);

        let decoded = codec.decode(&expected_bytes).expect("decode succeeds");
        assert_eq!(decoded.canvas, spec);
        assert_eq!(decoded.font_name, "default");
        assert_eq!(decoded.frame.background, Rgb::BLACK);
        assert_eq!(
            decoded.frame.grid.get(CellCoord::new(0, 0)),
            image.frame.grid.get(CellCoord::new(0, 0))
        );
        assert_eq!(
            decoded.frame.grid.get(CellCoord::new(1, 0)),
            image.frame.grid.get(CellCoord::new(1, 0))
        );
    }

    #[test]
    fn test_test_vector_08d_layered() {
        // Test vector from diagram 08d:
        // Frame background #101020, active layer = 1, 2 layers (bottom first).
        // Layer 0 "base": visible, opacity 255. cells: 'H' white on nothing | 'i' red on blue at alpha 0x80.
        // Layer 1 "top": HIDDEN, opacity 0x80. cells: no character | 'X' green (opaque). Fill plane empty.
        let spec = CanvasSpec {
            cols: 2,
            rows: 1,
            cell_w: 8,
            cell_h: 15,
        };

        let mut layer0 = Layer::blank(LayerId(1), "base".to_string(), &spec);
        layer0.visible = true;
        layer0.opacity = 255;
        layer0.ink.set(
            CellCoord::new(0, 0),
            InkCell::new('H', Rgba::WHITE),
        );
        layer0.ink.set(
            CellCoord::new(1, 0),
            InkCell::new('i', Rgba::new(255, 0, 0, 255)),
        );
        layer0.fill.set(
            CellCoord::new(1, 0),
            Rgba::new(0, 0, 255, 0x80),
        );

        let mut layer1 = Layer::blank(LayerId(2), "top".to_string(), &spec);
        layer1.visible = false;
        layer1.opacity = 0x80;
        layer1.ink.set(
            CellCoord::new(1, 0),
            InkCell::new('X', Rgba::new(0, 255, 0, 255)),
        );

        let frame = Frame::from_raw_parts(
            FrameId(1),
            Rgb::new(0x10, 0x10, 0x20),
            vec![layer0, layer1],
            1, // active layer = 1
            3, // next layer id = 3
        )
        .unwrap();

        let expected_bytes: [u8; 70] = [
            // FrameHeader (7 bytes)
            0x10, 0x10, 0x20, // background
            0x01, 0x00,       // active_layer = 1
            0x02, 0x00,       // layer_count = 2
            // LayerBlock 0 (32 bytes)
            0x04, 0x00,       // name len = 4
            0x62, 0x61, 0x73, 0x65, // "base"
            0x01,             // flags = visible
            0xFF,             // opacity = 255
            0x48, 0x00, 0x00, 0x00, // glyph 0 = 'H'
            0x69, 0x00, 0x00, 0x00, // glyph 1 = 'i'
            0xFF, 0xFF, 0xFF, 0xFF, // ink 0 = white
            0xFF, 0x00, 0x00, 0xFF, // ink 1 = red
            0x00, 0x00, 0x00, 0x00, // fill 0 = nothing
            0x00, 0x00, 0xFF, 0x80, // fill 1 = blue 0x80
            // LayerBlock 1 (31 bytes)
            0x03, 0x00,       // name len = 3
            0x74, 0x6F, 0x70, // "top"
            0x00,             // flags = hidden
            0x80,             // opacity = 128
            0x20, 0x00, 0x00, 0x00, // glyph 0 = ' '
            0x58, 0x00, 0x00, 0x00, // glyph 1 = 'X'
            0x00, 0x00, 0x00, 0x00, // ink 0 = empty
            0x00, 0xFF, 0x00, 0xFF, // ink 1 = green
            0x00, 0x00, 0x00, 0x00, // fill 0 = nothing
            0x00, 0x00, 0x00, 0x00, // fill 1 = nothing
        ];

        let mut out = Vec::new();
        LayeredFrameBlockCodec::write(&mut out, &frame);
        assert_eq!(out, expected_bytes);

        let mut r = ByteReader::new(&expected_bytes);
        let read_frame = LayeredFrameBlockCodec::read(&mut r, &spec, FrameId(1)).unwrap();
        assert_eq!(read_frame.background, Rgb::new(0x10, 0x10, 0x20));
        assert_eq!(read_frame.active_index(), 1);
        assert_eq!(read_frame.layers().len(), 2);
        assert_eq!(read_frame.layers()[0].id, LayerId(1));
        assert_eq!(read_frame.layers()[1].id, LayerId(2));
    }

    #[test]
    fn test_project_aspr_encode_decode() {
        let now = 1000200030;
        let mut proj = Project::new("MyProject".to_string(), Rgb::new(20, 30, 40), Fps(24), now);
        let active_layer_id = proj.timeline.selected_frame().active_layer().id;
        proj.timeline.selected_frame_mut().set_ink(
            active_layer_id,
            CellCoord::new(5, 5),
            InkCell::new('Z', Rgba::new(100, 150, 200, 255)),
        );
        proj.timeline.selected_frame_mut().set_fill(
            active_layer_id,
            CellCoord::new(5, 5),
            Rgba::new(10, 20, 30, 255),
        );

        let codec = AsprCodec;
        let encoded = codec.encode(&proj).expect("aspr encode succeeds");
        let decoded = codec.decode(&encoded).expect("aspr decode succeeds");

        assert_eq!(decoded.meta.name, "MyProject");
        assert_eq!(decoded.meta.created_at, now);
        assert_eq!(decoded.meta.modified_at, now);
        assert_eq!(decoded.timeline.fps, Fps(24));
        assert_eq!(decoded.timeline.len(), 1);
        assert_eq!(decoded.timeline.selected_index(), 0);
        let decoded_cell = decoded
            .timeline
            .selected_frame()
            .active_layer()
            .ink
            .get(CellCoord::new(5, 5))
            .unwrap();
        assert_eq!(decoded_cell.glyph, 'Z');
        assert_eq!(decoded_cell.color, Rgba::new(100, 150, 200, 255));
    }

    #[test]
    fn test_text_importer() {
        let text = b"\xEF\xBB\xBFHello\tWorld!\r\nLine 2";
        let parsed = TextImporter::parse(text).unwrap();
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.expanded_tabs, 1);
    }

    #[test]
    fn test_brush_store() {
        let temp_dir = std::env::temp_dir().join(format!("ascii_brush_test_{}", Clock::now_unix()));
        let brush_file = temp_dir.join("brushes.json");
        let mut store = BrushStore::with_path(brush_file.clone());

        let custom = Brush {
            id: BrushId(0),
            name: "Star".to_string(),
            stamp: Stamp::new_single('*'),
        };
        let id = store.upsert(custom).unwrap();
        assert!(id.0 >= 1000);

        // Reload
        let reloaded = BrushStore::with_path(brush_file);
        assert!(reloaded.library().get(id).is_some());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
