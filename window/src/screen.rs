use crate::ScreenRect;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Screens {
    pub main: ScreenInfo,
    pub active: ScreenInfo,
    pub by_name: HashMap<String, ScreenInfo>,
    pub virtual_rect: ScreenRect,
}

#[derive(Debug, Clone)]
pub struct ScreenInfo {
    pub name: String,
    pub rect: ScreenRect,
    pub scale: f64,
    pub max_fps: Option<usize>,
    pub effective_dpi: Option<f64>,
}

// --- weezterm remote features ---
/// Compute a map from monitor name to grid position label.
///
/// Monitors are grouped into rows and columns based on overlap of their
/// screen rectangles (using a tolerance to handle slight misalignments).
/// Each monitor is then assigned a human-readable label:
///
/// - 2×2: `"top-left"`, `"top-right"`, `"bottom-left"`, `"bottom-right"`
/// - side-by-side (1 row): `"left"`, `"right"` or `"left"`, `"center"`, `"right"`
/// - stacked (1 col): `"top"`, `"bottom"` or `"top"`, `"middle"`, `"bottom"`
/// - other: `"row0-col0"`, `"row0-col1"`, etc.
pub fn compute_monitor_positions(
    monitors: &HashMap<String, ScreenInfo>,
) -> HashMap<String, String> {
    if monitors.is_empty() {
        return HashMap::new();
    }

    // Collect (name, x, y, w, h) for each monitor
    let mut items: Vec<(&str, isize, isize, isize, isize)> = monitors
        .iter()
        .map(|(name, info)| {
            (
                name.as_str(),
                info.rect.origin.x,
                info.rect.origin.y,
                info.rect.size.width,
                info.rect.size.height,
            )
        })
        .collect();
    items.sort_by_key(|&(_, x, y, _, _)| (y, x));

    // Cluster into rows: two monitors are in the same row if they have
    // substantial vertical overlap (>50% of the shorter monitor's height).
    let mut row_clusters: Vec<Vec<usize>> = Vec::new();
    for idx in 0..items.len() {
        let (_, _, y, _, h) = items[idx];
        let mut placed = false;
        for cluster in row_clusters.iter_mut() {
            // Compare with first member of cluster
            let rep = cluster[0];
            let (_, _, ry, _, rh) = items[rep];
            if vertical_overlap(y, h, ry, rh) {
                cluster.push(idx);
                placed = true;
                break;
            }
        }
        if !placed {
            row_clusters.push(vec![idx]);
        }
    }

    // Sort rows by min Y, sort monitors within each row by X
    row_clusters.sort_by_key(|cluster| {
        cluster.iter().map(|&i| items[i].1).min().unwrap_or(0) + // min Y for tiebreak
        cluster.iter().map(|&i| items[i].2).min().unwrap_or(0) * 100000 // min Y primary
    });
    // Actually sort by min Y of the row
    row_clusters.sort_by_key(|cluster| cluster.iter().map(|&i| items[i].2).min().unwrap_or(0));
    for cluster in row_clusters.iter_mut() {
        cluster.sort_by_key(|&i| items[i].1); // sort by X within row
    }

    let num_rows = row_clusters.len();
    let num_cols = row_clusters.iter().map(|c| c.len()).max().unwrap_or(1);

    let mut result = HashMap::new();

    for (row_idx, cluster) in row_clusters.iter().enumerate() {
        let cols_in_row = cluster.len();
        for (col_idx, &monitor_idx) in cluster.iter().enumerate() {
            let name = items[monitor_idx].0;
            let label = position_label(row_idx, col_idx, num_rows, num_cols, cols_in_row);
            result.insert(name.to_string(), label);
        }
    }

    result
}

/// Check if two vertical spans overlap by more than 50% of the shorter one.
fn vertical_overlap(y1: isize, h1: isize, y2: isize, h2: isize) -> bool {
    let top = y1.max(y2);
    let bottom = (y1 + h1).min(y2 + h2);
    let overlap = (bottom - top).max(0);
    let min_height = h1.min(h2).max(1);
    // >50% overlap of the shorter monitor
    overlap * 2 > min_height
}

/// Generate a human-readable position label for a monitor at (row, col)
/// given the total grid dimensions.
fn position_label(
    row: usize,
    col: usize,
    num_rows: usize,
    num_cols: usize,
    cols_in_row: usize,
) -> String {
    // Single monitor
    if num_rows == 1 && num_cols == 1 {
        return "sole".to_string();
    }

    let row_label = match num_rows {
        1 => None,
        2 => Some(if row == 0 { "top" } else { "bottom" }),
        3 => Some(match row {
            0 => "top",
            1 => "middle",
            _ => "bottom",
        }),
        _ => None, // fallback to row0/row1/... below
    };

    let col_label = match cols_in_row.max(num_cols) {
        1 => None,
        2 => Some(if col == 0 { "left" } else { "right" }),
        3 => Some(match col {
            0 => "left",
            1 => "center",
            _ => "right",
        }),
        _ => None, // fallback
    };

    match (row_label, col_label) {
        (Some(r), Some(c)) => format!("{}-{}", r, c),
        (Some(r), None) => r.to_string(),
        (None, Some(c)) => c.to_string(),
        (None, None) => format!("row{}-col{}", row, col),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use euclid::{Point2D, Size2D};

    fn make_screen(name: &str, x: isize, y: isize, w: isize, h: isize) -> (String, ScreenInfo) {
        (
            name.to_string(),
            ScreenInfo {
                name: name.to_string(),
                rect: ScreenRect::new(Point2D::new(x, y), Size2D::new(w, h)),
                scale: 1.0,
                max_fps: None,
                effective_dpi: None,
            },
        )
    }

    #[test]
    fn test_2x2_grid() {
        let monitors: HashMap<String, ScreenInfo> = vec![
            make_screen("TL", 0, 0, 1920, 1080),
            make_screen("TR", 1920, 0, 1920, 1080),
            make_screen("BL", 0, 1080, 1920, 1080),
            make_screen("BR", 1920, 1080, 1920, 1080),
        ]
        .into_iter()
        .collect();

        let pos = compute_monitor_positions(&monitors);
        assert_eq!(pos["TL"], "top-left");
        assert_eq!(pos["TR"], "top-right");
        assert_eq!(pos["BL"], "bottom-left");
        assert_eq!(pos["BR"], "bottom-right");
    }

    #[test]
    fn test_2x2_with_offset() {
        // Slight Y offset (10px) — should still cluster into same row
        let monitors: HashMap<String, ScreenInfo> = vec![
            make_screen("TL", 0, 0, 1920, 1080),
            make_screen("TR", 1920, 10, 1920, 1080),
            make_screen("BL", 0, 1080, 1920, 1080),
            make_screen("BR", 1925, 1090, 1920, 1080),
        ]
        .into_iter()
        .collect();

        let pos = compute_monitor_positions(&monitors);
        assert_eq!(pos["TL"], "top-left");
        assert_eq!(pos["TR"], "top-right");
        assert_eq!(pos["BL"], "bottom-left");
        assert_eq!(pos["BR"], "bottom-right");
    }

    #[test]
    fn test_side_by_side() {
        let monitors: HashMap<String, ScreenInfo> = vec![
            make_screen("L", 0, 0, 1920, 1080),
            make_screen("R", 1920, 0, 2560, 1440),
        ]
        .into_iter()
        .collect();

        let pos = compute_monitor_positions(&monitors);
        assert_eq!(pos["L"], "left");
        assert_eq!(pos["R"], "right");
    }

    #[test]
    fn test_stacked() {
        let monitors: HashMap<String, ScreenInfo> = vec![
            make_screen("T", 0, 0, 1920, 1080),
            make_screen("B", 0, 1080, 1920, 1080),
        ]
        .into_iter()
        .collect();

        let pos = compute_monitor_positions(&monitors);
        assert_eq!(pos["T"], "top");
        assert_eq!(pos["B"], "bottom");
    }

    #[test]
    fn test_single_monitor() {
        let monitors: HashMap<String, ScreenInfo> = vec![make_screen("M", 0, 0, 1920, 1080)]
            .into_iter()
            .collect();

        let pos = compute_monitor_positions(&monitors);
        assert_eq!(pos["M"], "sole");
    }

    #[test]
    fn test_three_side_by_side() {
        let monitors: HashMap<String, ScreenInfo> = vec![
            make_screen("L", 0, 0, 1920, 1080),
            make_screen("C", 1920, 0, 1920, 1080),
            make_screen("R", 3840, 0, 1920, 1080),
        ]
        .into_iter()
        .collect();

        let pos = compute_monitor_positions(&monitors);
        assert_eq!(pos["L"], "left");
        assert_eq!(pos["C"], "center");
        assert_eq!(pos["R"], "right");
    }

    #[test]
    fn test_negative_coordinates() {
        // Primary at 0,0, secondary to the left at negative X
        let monitors: HashMap<String, ScreenInfo> = vec![
            make_screen("R", 0, 0, 1920, 1080),
            make_screen("L", -1920, 0, 1920, 1080),
        ]
        .into_iter()
        .collect();

        let pos = compute_monitor_positions(&monitors);
        assert_eq!(pos["L"], "left");
        assert_eq!(pos["R"], "right");
    }
}
// --- end weezterm remote features ---
