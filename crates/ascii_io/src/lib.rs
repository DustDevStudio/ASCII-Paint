pub mod export;
pub mod format;
pub mod storage;

pub use export::{Clock, ExportError, ExportJobHandle, ExportMessage, ExportService, FfmpegLocator};
pub use format::{
    AanimCodec, AsciiAnimation, AsciiCodec, AsciiImage, AsprCodec, ByteReader, ContainerCodec,
    ContainerHeader, FileKind, FormatCodec, FormatError, FrameBlockCodec,
};
pub use storage::{AtomicWriter, ProjectRepository, RecentEntry, RecentStore, RepoError};

#[cfg(test)]
mod tests {
    use super::*;
    use ascii_core::model::{CanvasSpec, Cell, CellCoord, Fps, Frame, FrameId, Project, Rgb};

    #[test]
    fn test_test_vector_08c() {
        // Test vector from diagram 08c:
        // Image content: cell 0 = 'H' white on transparent, cell 1 = 'i' red on blue. Frame background #000000.
        // Canvas 2 x 1, cell 8 x 15, font_name "default".
        let spec = CanvasSpec {
            cols: 2,
            rows: 1,
            cell_w: 8,
            cell_h: 15,
        };
        let mut frame = Frame::blank(FrameId(1), &spec, Rgb::BLACK);
        frame.set_cell(
            CellCoord::new(0, 0),
            Cell {
                glyph: 'H',
                fg: Rgb::WHITE,
                bg: None,
            },
        );
        frame.set_cell(
            CellCoord::new(1, 0),
            Cell {
                glyph: 'i',
                fg: Rgb::new(255, 0, 0),
                bg: Some(Rgb::new(0, 0, 255)),
            },
        );

        let image = AsciiImage {
            canvas: spec,
            font_name: "default".to_string(),
            frame,
        };

        let expected_bytes: [u8; 60] = [
            0x41, 0x53, 0x43, 0x49, // "ASCI"
            0x01, 0x00,             // version = 1
            0x00, 0x00,             // flags = 0
            0x28, 0x00, 0x00, 0x00, // uncompressed_len = 40
            0x28, 0x00, 0x00, 0x00, // stored_len = 40
            0x81, 0x23, 0x19, 0xDA, // crc32 = 0xDA192381
            // CanvasHeader (15 bytes)
            0x02, 0x00,             // cols = 2
            0x01, 0x00,             // rows = 1
            0x08,                   // cell_w = 8
            0x0F,                   // cell_h = 15
            0x07, 0x00,             // string len = 7
            0x64, 0x65, 0x66, 0x61, 0x75, 0x6C, 0x74, // "default"
            // FrameBlock (25 bytes)
            0x00, 0x00, 0x00,       // background = #000000
            0x48, 0x00, 0x00, 0x00, // glyph 0 = 'H'
            0x69, 0x00, 0x00, 0x00, // glyph 1 = 'i'
            0xFF, 0xFF, 0xFF,       // fg 0 = #FFFFFF
            0xFF, 0x00, 0x00,       // fg 1 = #FF0000
            0x00, 0x00, 0x00, 0x00, // bg 0 = None (alpha = 0)
            0x00, 0x00, 0xFF, 0xFF, // bg 1 = Some(#0000FF) (alpha = 255)
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
    fn test_project_aspr_encode_decode() {
        let now = 1000200030;
        let mut proj = Project::new("MyProject".to_string(), Rgb::new(20, 30, 40), Fps(24), now);
        proj.timeline.selected_frame_mut().set_cell(
            CellCoord::new(5, 5),
            Cell {
                glyph: 'Z',
                fg: Rgb::new(100, 150, 200),
                bg: Some(Rgb::new(10, 20, 30)),
            },
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
            .grid
            .get(CellCoord::new(5, 5))
            .unwrap();
        assert_eq!(decoded_cell.glyph, 'Z');
        assert_eq!(decoded_cell.fg, Rgb::new(100, 150, 200));
        assert_eq!(decoded_cell.bg, Some(Rgb::new(10, 20, 30)));
    }

    #[test]
    fn test_recent_store() {
        let temp_dir = std::env::temp_dir().join(format!("ascii_recent_test_{}", Clock::now_unix()));
        let config_file = temp_dir.join("recent.json");
        let mut store = RecentStore::with_path(config_file.clone());

        assert!(store.entries().is_empty());
        store.touch(temp_dir.join("p1.aspr"), "P1".to_string(), 100);
        store.touch(temp_dir.join("p2.aspr"), "P2".to_string(), 200);

        assert_eq!(store.entries().len(), 2);
        assert_eq!(store.entries()[0].name, "P2");
        assert_eq!(store.entries()[1].name, "P1");

        // Reload
        let reloaded = RecentStore::with_path(config_file);
        assert_eq!(reloaded.entries().len(), 2);
        assert_eq!(reloaded.entries()[0].name, "P2");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
