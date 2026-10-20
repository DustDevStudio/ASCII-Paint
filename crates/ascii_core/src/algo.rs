use std::collections::BTreeSet;

use crate::model::{CanvasSpec, CellCoord, CellRect, Layer, Plane};
use crate::selection::Selection;

pub struct FloodFill;

impl FloodFill {
    pub fn region(
        layer: &Layer,
        seed: CellCoord,
        plane: Plane,
        limit: &Selection,
    ) -> Vec<CellCoord> {
        let cols = layer.ink.cols() as usize;
        let rows = layer.ink.rows() as usize;
        if seed.col as usize >= cols || seed.row as usize >= rows {
            return Vec::new();
        }

        let target = match layer.value(plane, seed) {
            Some(v) => v,
            None => return Vec::new(),
        };

        let mut visited = vec![false; cols * rows];
        let mut result = Vec::new();
        let mut stack = vec![(seed.col as usize, seed.row as usize)];

        let value_matches = |c: CellCoord| -> bool {
            layer.value(plane, c) == Some(target)
        };

        let ok = |x: usize, y: usize, visited: &[bool]| -> bool {
            if x >= cols || y >= rows {
                return false;
            }
            let coord = CellCoord::new(x as u16, y as u16);
            !visited[y * cols + x] && limit.permits(coord) && value_matches(coord)
        };

        while let Some((x, y)) = stack.pop() {
            let coord = CellCoord::new(x as u16, y as u16);
            if visited[y * cols + x] || !limit.permits(coord) || !value_matches(coord) {
                continue;
            }

            let mut xl = x;
            while xl > 0 && ok(xl - 1, y, &visited) {
                xl -= 1;
            }
            let mut xr = x;
            while xr + 1 < cols && ok(xr + 1, y, &visited) {
                xr += 1;
            }

            for i in xl..=xr {
                visited[y * cols + i] = true;
                result.push(CellCoord::new(i as u16, y as u16));
            }

            let mut neighbor_rows = Vec::new();
            if y > 0 {
                neighbor_rows.push(y - 1);
            }
            if y + 1 < rows {
                neighbor_rows.push(y + 1);
            }

            for ny in neighbor_rows {
                let mut i = xl;
                while i <= xr {
                    if ok(i, ny, &visited) && (i == xl || !ok(i - 1, ny, &visited)) {
                        stack.push((i, ny));
                    }
                    i += 1;
                }
            }
        }

        result.sort_by_key(|c| (c.row, c.col));
        result
    }
}

pub struct ShapeRasterizer;

impl ShapeRasterizer {
    pub fn line(a: CellCoord, b: CellCoord) -> Vec<CellCoord> {
        let mut x0 = a.col as i32;
        let mut y0 = a.row as i32;
        let x1 = b.col as i32;
        let y1 = b.row as i32;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut out = Vec::new();
        out.push(CellCoord::new(x0 as u16, y0 as u16));

        while (x0, y0) != (x1, y1) {
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
            out.push(CellCoord::new(x0 as u16, y0 as u16));
        }
        out
    }

    pub fn rect(a: CellCoord, b: CellCoord, spec: &CanvasSpec) -> Vec<CellCoord> {
        let r = CellRect::from_corners(a, b);
        let max_col = spec.cols.saturating_sub(1);
        let max_row = spec.rows.saturating_sub(1);
        let clamped = CellRect {
            min: CellCoord::new(r.min.col.min(max_col), r.min.row.min(max_row)),
            max: CellCoord::new(r.max.col.min(max_col), r.max.row.min(max_row)),
        };
        clamped.iter().collect()
    }

    pub fn ellipse(a: CellCoord, b: CellCoord, spec: &CanvasSpec) -> Vec<CellCoord> {
        let r = CellRect::from_corners(a, b);
        let max_col = spec.cols.saturating_sub(1);
        let max_row = spec.rows.saturating_sub(1);
        let min_col = r.min.col.min(max_col);
        let min_row = r.min.row.min(max_row);
        let max_c = r.max.col.min(max_col);
        let max_r = r.max.row.min(max_row);

        let w = (max_c.saturating_sub(min_col) + 1) as f64;
        let h = (max_r.saturating_sub(min_row) + 1) as f64;
        let cx = min_col as f64 + w / 2.0;
        let cy = min_row as f64 + h / 2.0;
        let rx = w / 2.0;
        let ry = h / 2.0;

        let mut out = Vec::new();
        for r0 in min_row..=max_r {
            for c in min_col..=max_c {
                let dx = ((c as f64 + 0.5) - cx) / rx;
                let dy = ((r0 as f64 + 0.5) - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    out.push(CellCoord::new(c, r0));
                }
            }
        }
        out
    }

    pub fn polygon(vertices: &[CellCoord], spec: &CanvasSpec) -> Vec<CellCoord> {
        let n = vertices.len();
        if n < 3 {
            return Vec::new();
        }

        let mut out_set = BTreeSet::new();
        let min_row = vertices.iter().map(|v| v.row).min().unwrap_or(0);
        let max_row = vertices.iter().map(|v| v.row).max().unwrap_or(0);

        for r in min_row..=max_row {
            let y = r as f64 + 0.5;
            let mut xs = Vec::new();

            for i in 0..n {
                let p = vertices[i];
                let q = vertices[(i + 1) % n];
                if p.row != q.row {
                    let y1 = p.row as f64 + 0.5;
                    let y2 = q.row as f64 + 0.5;
                    let x1 = p.col as f64 + 0.5;
                    let x2 = q.col as f64 + 0.5;

                    if (y1 <= y && y < y2) || (y2 <= y && y < y1) {
                        let x = x1 + (y - y1) * (x2 - x1) / (y2 - y1);
                        xs.push(x);
                    }
                }
            }

            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mut i = 0;
            while i + 1 < xs.len() {
                let xa = xs[i];
                let xb = xs[i + 1];
                let start_col = xa.floor() as i32;
                let end_col = xb.ceil() as i32;
                for c in start_col..=end_col {
                    let center = c as f64 + 0.5;
                    if xa <= center && center < xb {
                        if c >= 0 && (c as u16) < spec.cols && r < spec.rows {
                            out_set.insert(CellCoord::new(c as u16, r));
                        }
                    }
                }
                i += 2;
            }
        }

        // Outline: for each edge line(v[i], v[(i+1)%n])
        for i in 0..n {
            let p = vertices[i];
            let q = vertices[(i + 1) % n];
            for pt in Self::line(p, q) {
                if spec.contains(pt) {
                    out_set.insert(pt);
                }
            }
        }

        let mut res: Vec<CellCoord> = out_set.into_iter().collect();
        res.sort_by_key(|c| (c.row, c.col));
        res
    }
}
