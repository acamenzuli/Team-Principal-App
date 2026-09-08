//! Virtual desktop layout: the bounding box, and the dead regions inside it.
//!
//! Windows lays every monitor out in one coordinate space whose origin is the
//! primary monitor's top-left. Monitors to the left of or above it therefore
//! have **negative** coordinates, and the overall desktop is the bounding box
//! of all of them.
//!
//! That box is not the same shape as the monitors. On a rig with panels of
//! different heights — a 1440-tall centre flanked by 1080-tall sides, say —
//! parts of the box map to no physical panel at all. A window placed there is
//! simply invisible. The layout editor draws these regions, and a span-mode
//! rectangle has to account for them, so they are computed here rather than
//! guessed at by eye.
//!
//! Pure math over rectangles. No Win32: the real provider and the mock both
//! hand this the same list.

use tp_model::PixelRect;

/// The result of laying out a set of monitors in virtual desktop space.
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopLayout {
    /// The full bounding box, including any dead space.
    pub bounds: PixelRect,
    /// Regions of `bounds` that map to no monitor, as a set of disjoint
    /// rectangles. Empty when the monitors tile the box exactly.
    pub dead_regions: Vec<PixelRect>,
    /// Sum of the monitors' own areas, in pixels.
    pub covered_area: u64,
}

impl DesktopLayout {
    pub fn dead_area(&self) -> u64 {
        self.dead_regions.iter().map(area).sum()
    }

    /// True when every pixel of the bounding box is on a real panel.
    pub fn is_gapless(&self) -> bool {
        self.dead_regions.is_empty()
    }
}

/// Lay out monitors and find the dead regions.
///
/// Monitors are assumed non-overlapping, which Windows guarantees. Overlap
/// would not crash this, but `covered_area` would double-count.
pub fn desktop_layout(monitors: &[PixelRect]) -> Option<DesktopLayout> {
    if monitors.is_empty() {
        return None;
    }

    let left = monitors.iter().map(|m| m.x).min()?;
    let top = monitors.iter().map(|m| m.y).min()?;
    let right = monitors.iter().map(|m| m.right()).max()?;
    let bottom = monitors.iter().map(|m| m.bottom()).max()?;

    let bounds = PixelRect {
        x: left,
        y: top,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    };

    // Coordinate compression: every monitor edge becomes a grid line, so each
    // resulting cell is either wholly covered or wholly empty. With a handful
    // of monitors this is a grid of at most a few dozen cells.
    let mut xs: Vec<i32> = monitors.iter().flat_map(|m| [m.x, m.right()]).collect();
    let mut ys: Vec<i32> = monitors.iter().flat_map(|m| [m.y, m.bottom()]).collect();
    xs.extend([left, right]);
    ys.extend([top, bottom]);
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();

    let mut uncovered: Vec<PixelRect> = Vec::new();
    for row in ys.windows(2) {
        let (y0, y1) = (row[0], row[1]);
        if y0 == y1 {
            continue;
        }
        // Collect this row's uncovered cells, merging horizontally as we go.
        let mut run: Option<PixelRect> = None;
        for col in xs.windows(2) {
            let (x0, x1) = (col[0], col[1]);
            if x0 == x1 {
                continue;
            }
            let covered = monitors
                .iter()
                .any(|m| m.x <= x0 && m.right() >= x1 && m.y <= y0 && m.bottom() >= y1);

            match (&mut run, covered) {
                (Some(r), false) if r.right() == x0 => r.width += (x1 - x0) as u32,
                (_, false) => {
                    if let Some(r) = run.take() {
                        uncovered.push(r);
                    }
                    run = Some(PixelRect {
                        x: x0,
                        y: y0,
                        width: (x1 - x0) as u32,
                        height: (y1 - y0) as u32,
                    });
                }
                (_, true) => {
                    if let Some(r) = run.take() {
                        uncovered.push(r);
                    }
                }
            }
        }
        if let Some(r) = run.take() {
            uncovered.push(r);
        }
    }

    Some(DesktopLayout {
        bounds,
        dead_regions: merge_vertically(uncovered),
        covered_area: monitors.iter().map(area).sum(),
    })
}

/// Join rectangles that share an x-span and touch vertically, so a dead strip
/// spanning several row bands is reported as one region rather than four.
fn merge_vertically(mut rects: Vec<PixelRect>) -> Vec<PixelRect> {
    rects.sort_by_key(|r| (r.x, r.width, r.y));
    let mut out: Vec<PixelRect> = Vec::with_capacity(rects.len());
    for r in rects {
        match out.last_mut() {
            Some(prev) if prev.x == r.x && prev.width == r.width && prev.bottom() == r.y => {
                prev.height += r.height;
            }
            _ => out.push(r),
        }
    }
    out
}

fn area(r: &PixelRect) -> u64 {
    r.width as u64 * r.height as u64
}

/// Pixels per millimetre for a panel. `None` when the physical size is unknown
/// or zero — the caller must not invent one.
pub fn pixel_pitch(width_px: u32, width_mm: f64) -> Option<f64> {
    (width_mm > 0.0).then(|| width_px as f64 / width_mm)
}

/// Relative difference in pixel pitch across a seam, as a fraction.
///
/// This is what decides whether a bezel gap can be expressed in pixels at all.
/// It only can when both panels have the same pitch; otherwise the conversion
/// is meaningless and the app says so instead of returning a wrong number.
pub fn pitch_mismatch(a: f64, b: f64) -> f64 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    if lo <= 0.0 {
        return f64::INFINITY;
    }
    (hi - lo) / lo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: u32, h: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    /// The invariant that makes the whole thing trustworthy: the bounding box
    /// is exactly the monitors plus the dead regions, with nothing double
    /// counted and nothing lost.
    fn assert_partitions(layout: &DesktopLayout) {
        let box_area = layout.bounds.width as u64 * layout.bounds.height as u64;
        assert_eq!(
            box_area,
            layout.covered_area + layout.dead_area(),
            "bounding box {box_area} != covered {} + dead {}",
            layout.covered_area,
            layout.dead_area()
        );
        // And the dead regions must not overlap each other.
        for (i, a) in layout.dead_regions.iter().enumerate() {
            for b in &layout.dead_regions[i + 1..] {
                let overlap_x = a.x.max(b.x) < a.right().min(b.right());
                let overlap_y = a.y.max(b.y) < a.bottom().min(b.bottom());
                assert!(
                    !(overlap_x && overlap_y),
                    "dead regions {a:?} and {b:?} overlap"
                );
            }
        }
    }

    #[test]
    fn identical_panels_on_a_baseline_have_no_dead_space() {
        let l = desktop_layout(&[
            r(-2560, 0, 2560, 1440),
            r(0, 0, 2560, 1440),
            r(2560, 0, 2560, 1440),
        ])
        .unwrap();
        assert_eq!(l.bounds, r(-2560, 0, 7680, 1440));
        assert!(l.is_gapless());
        assert_partitions(&l);
    }

    #[test]
    fn a_single_monitor_is_its_own_desktop() {
        let l = desktop_layout(&[r(0, 0, 5120, 1440)]).unwrap();
        assert_eq!(l.bounds, r(0, 0, 5120, 1440));
        assert!(l.is_gapless());
        assert_partitions(&l);
    }

    #[test]
    fn no_monitors_is_none_rather_than_an_empty_desktop() {
        assert!(desktop_layout(&[]).is_none());
    }

    #[test]
    fn negative_coordinates_are_handled() {
        // Monitors left of and above the primary. Getting this wrong is the
        // classic multi-monitor bug: everything works until the user puts a
        // screen on the left.
        let l = desktop_layout(&[r(-1920, -1080, 1920, 1080), r(0, 0, 2560, 1440)]).unwrap();
        assert_eq!(l.bounds, r(-1920, -1080, 4480, 2520));
        assert_partitions(&l);
    }

    #[test]
    fn mismatched_heights_produce_dead_regions() {
        // The rig this product exists for: a 1440-tall centre between two
        // 1080-tall sides, sides top-aligned. The strip under each side panel
        // is virtual desktop space that maps to no glass.
        let l = desktop_layout(&[
            r(-1920, 0, 1920, 1080),
            r(0, 0, 5120, 1440),
            r(5120, 0, 1920, 1080),
        ])
        .unwrap();

        assert_eq!(l.bounds, r(-1920, 0, 8960, 1440));
        assert_eq!(l.dead_regions.len(), 2, "one strip under each side panel");
        assert_partitions(&l);

        let mut dead = l.dead_regions.clone();
        dead.sort_by_key(|d| d.x);
        assert_eq!(dead[0], r(-1920, 1080, 1920, 360));
        assert_eq!(dead[1], r(5120, 1080, 1920, 360));
    }

    #[test]
    fn a_vertically_centred_side_panel_leaves_dead_space_above_and_below() {
        // Sides centred on the taller centre panel: 180 px dead at top and
        // bottom of each. Four regions, and the merge must not fuse the top
        // strip of one panel with the bottom strip of another.
        let l = desktop_layout(&[
            r(-1920, 180, 1920, 1080),
            r(0, 0, 5120, 1440),
            r(5120, 180, 1920, 1080),
        ])
        .unwrap();

        assert_eq!(l.dead_regions.len(), 4);
        assert_partitions(&l);
        assert!(l.dead_regions.iter().all(|d| d.height == 180));
    }

    #[test]
    fn a_gap_between_two_monitors_is_dead_space() {
        // Windows allows monitors not to touch. The space between them is
        // addressable and invisible.
        let l = desktop_layout(&[r(0, 0, 1920, 1080), r(2400, 0, 1920, 1080)]).unwrap();
        assert_eq!(l.dead_regions, vec![r(1920, 0, 480, 1080)]);
        assert_partitions(&l);
    }

    #[test]
    fn a_dead_strip_spanning_several_bands_is_reported_as_one_region() {
        // Three stacked monitors of decreasing width leave a staircase to the
        // right. Without vertical merging this reports more regions than
        // there are, and the layout editor draws seams that do not exist.
        let l = desktop_layout(&[
            r(0, 0, 1000, 100),
            r(0, 100, 1000, 100),
            r(0, 200, 800, 100),
        ])
        .unwrap();
        assert_eq!(l.bounds, r(0, 0, 1000, 300));
        assert_eq!(l.dead_regions, vec![r(800, 200, 200, 100)]);
        assert_partitions(&l);
    }

    #[test]
    fn pitch_tells_you_when_a_bezel_gap_cannot_be_pixels() {
        // A 5120x1440 49" and a 2560x1440 27" are the same pitch to within a
        // fraction of a percent — the 49" is two 27" panels in one chassis.
        let ultrawide = pixel_pitch(5120, 1193.0).unwrap();
        let p27_1440 = pixel_pitch(2560, 597.0).unwrap();
        assert!(pitch_mismatch(ultrawide, p27_1440) < 0.01);

        // Against a 1080p panel of the same physical size, it is about a third.
        let p27_1080 = pixel_pitch(1920, 597.0).unwrap();
        let mismatch = pitch_mismatch(ultrawide, p27_1080);
        assert!((0.30..0.40).contains(&mismatch), "mismatch {mismatch}");

        assert_eq!(pixel_pitch(2560, 0.0), None);
    }
}
