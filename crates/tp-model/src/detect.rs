//! Turning detected monitors into a starter rig.
//!
//! This is the difference between a product and a form. Everything Windows and
//! EDID can answer is filled in: which monitors exist, how they are arranged,
//! their native resolution, their physical size in millimetres. What is left is
//! only what no API can know — bezel thickness, screen angle, and how far back
//! the driver sits.
//!
//! Re-detecting must never discard those measurements. Screens are matched by
//! EDID identity rather than by position or device path, so unplugging a
//! monitor, swapping two cables, or changing the Windows arrangement leaves the
//! numbers someone measured with a tape measure exactly where they were.

use crate::{
    Curvature, Measurement, MonitorBinding, MonitorInfo, PanelSpec, RigModel, ScreenId, ScreenRole,
    ScreenSpec, WidthMeasure, RIG_SCHEMA_VERSION,
};

/// Build a rig from detected monitors, carrying over anything already measured.
pub fn rig_from_monitors(
    monitors: &[MonitorInfo],
    existing: Option<&RigModel>,
    now: &str,
) -> RigModel {
    let mut rig = match existing {
        Some(e) => {
            let mut r = e.clone();
            r.updated_at = now.to_string();
            r
        }
        None => RigModel::new("My rig", now),
    };
    rig.schema_version = RIG_SCHEMA_VERSION;

    if monitors.is_empty() {
        // Keep whatever was described rather than wiping a rig because a cable
        // is out. An unplugged monitor is a temporary state, not a decision.
        return rig;
    }

    // Left to right as physically arranged.
    let mut ordered: Vec<&MonitorInfo> = monitors.iter().collect();
    ordered.sort_by_key(|m| m.bounds.x);

    // The centre is the primary monitor when there is one, otherwise the widest
    // — which on a sim rig is the same panel almost every time.
    let centre_index = ordered
        .iter()
        .position(|m| m.is_primary)
        .unwrap_or_else(|| {
            ordered
                .iter()
                .enumerate()
                .max_by_key(|(_, m)| m.bounds.width)
                .map(|(i, _)| i)
                .unwrap_or(0)
        });

    rig.screens = ordered
        .iter()
        .enumerate()
        .map(|(index, monitor)| {
            let role = match index.cmp(&centre_index) {
                std::cmp::Ordering::Less => ScreenRole::Left,
                std::cmp::Ordering::Equal => ScreenRole::Center,
                std::cmp::Ordering::Greater => ScreenRole::Right,
            };
            let previous = existing.and_then(|e| find_by_identity(e, monitor));
            build_screen(ScreenId(index as u32 + 1), role, monitor, previous)
        })
        .collect();

    rig
}

/// Match by EDID identity, not by device path or position.
///
/// The device path encodes the adapter and output, so it changes when a cable
/// moves between ports; position changes whenever the Windows arrangement does.
/// Neither is a reason to lose someone's bezel measurements.
fn find_by_identity<'a>(rig: &'a RigModel, monitor: &MonitorInfo) -> Option<&'a ScreenSpec> {
    rig.screens.iter().find(|s| match &s.binding {
        MonitorBinding::Edid(id) => {
            id.manufacturer_id == monitor.identity.manufacturer_id
                && id.product_code == monitor.identity.product_code
                && id.serial == monitor.identity.serial
                && id.serial_number == monitor.identity.serial_number
        }
        MonitorBinding::Unbound => false,
    })
}

fn build_screen(
    id: ScreenId,
    role: ScreenRole,
    monitor: &MonitorInfo,
    previous: Option<&ScreenSpec>,
) -> ScreenSpec {
    // Physical size: EDID when it has one, otherwise whatever was measured
    // before, otherwise nothing. Never a zero — a zero would sail into the FOV
    // calculator and produce a nonsense angle instead of a question.
    let (width, height) = match monitor.physical_size {
        Some(size) => (
            Measurement::edid(size.width.0),
            Measurement::edid(size.height.0),
        ),
        None => match previous {
            Some(p) => (p.panel.visible_width, p.panel.visible_height),
            None => (Measurement::derived(0.0), Measurement::derived(0.0)),
        },
    };

    // A measurement the user typed always beats a re-detected EDID one: they
    // overrode it for a reason, usually because they measured the glass.
    let (width, height) = match previous {
        Some(p) if p.panel.visible_width.source == crate::MeasurementSource::Manual => {
            (p.panel.visible_width, p.panel.visible_height)
        }
        _ => (width, height),
    };

    ScreenSpec {
        id,
        role: previous.map(|p| p.role.clone()).unwrap_or(role),
        binding: MonitorBinding::Edid(crate::EdidIdentity {
            cached_device_path: Some(monitor.device_path.clone()),
            ..monitor.identity.clone()
        }),
        panel: PanelSpec {
            native_resolution: monitor.native_resolution,
            visible_width: width,
            visible_height: height,
            // None of these can be detected. Carrying them forward is the whole
            // point of matching by identity.
            width_measure: previous
                .map(|p| p.panel.width_measure)
                .unwrap_or(WidthMeasure::Arc),
            curvature: previous
                .map(|p| p.panel.curvature)
                .unwrap_or(Curvature::Flat),
            bezel: previous.map(|p| p.panel.bezel).unwrap_or_default(),
        },
        mounting: previous.map(|p| p.mounting).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bezel, Deg, DisplayMode, EdidIdentity, Mm, PhysicalSize, PixelRect, Resolution};

    fn monitor(
        x: i32,
        width: u32,
        serial: &str,
        primary: bool,
        size_mm: Option<(f64, f64)>,
    ) -> MonitorInfo {
        MonitorInfo {
            device_path: format!(r"\\?\DISPLAY#TST0001#{serial}#{{guid}}"),
            gdi_name: format!(r"\\.\DISPLAY{}", x.abs() % 9 + 1),
            friendly_name: format!("Panel {serial}"),
            identity: EdidIdentity {
                manufacturer_id: "TST".into(),
                product_code: 1,
                serial: Some(serial.into()),
                serial_number: 0,
                week_year: (1, 2024),
                cached_device_path: None,
            },
            native_resolution: Resolution {
                width,
                height: 1440,
            },
            current_mode: DisplayMode {
                resolution: Resolution {
                    width,
                    height: 1440,
                },
                refresh_hz: 144,
                bits_per_pixel: 32,
            },
            bounds: PixelRect {
                x,
                y: 0,
                width,
                height: 1440,
            },
            is_primary: primary,
            dpi_scale: 1.0,
            physical_size: size_mm.map(|(w, h)| PhysicalSize {
                width: Mm(w),
                height: Mm(h),
                millimetre_precision: true,
            }),
        }
    }

    fn triple() -> Vec<MonitorInfo> {
        vec![
            monitor(-2560, 2560, "L", false, Some((597.7, 336.2))),
            monitor(0, 5120, "C", true, Some((1193.0, 336.0))),
            monitor(5120, 2560, "R", false, Some((597.7, 336.2))),
        ]
    }

    #[test]
    fn roles_follow_the_physical_arrangement() {
        let rig = rig_from_monitors(&triple(), None, "now");
        let roles: Vec<&ScreenRole> = rig.screens.iter().map(|s| &s.role).collect();
        assert_eq!(
            roles,
            vec![&ScreenRole::Left, &ScreenRole::Center, &ScreenRole::Right]
        );
    }

    #[test]
    fn physical_size_comes_from_edid_and_says_so() {
        let rig = rig_from_monitors(&triple(), None, "now");
        let centre = rig.center().unwrap();
        assert_eq!(centre.panel.visible_width.mm, Mm(1193.0));
        assert_eq!(
            centre.panel.visible_width.source,
            crate::MeasurementSource::Edid
        );
        assert_eq!(centre.panel.native_resolution.width, 5120);
    }

    #[test]
    fn a_monitor_with_no_reported_size_is_zero_not_invented() {
        let rig = rig_from_monitors(&[monitor(0, 2560, "X", true, None)], None, "now");
        let s = rig.center().unwrap();
        assert_eq!(s.panel.visible_width.mm, Mm(0.0));
        assert_eq!(
            s.panel.visible_width.source,
            crate::MeasurementSource::Derived
        );
    }

    #[test]
    fn re_detecting_keeps_everything_that_was_measured() {
        // The property this whole module exists for: someone spends twenty
        // minutes with a tape measure and a piece of string, and plugging a
        // monitor back in must not throw it away.
        let mut rig = rig_from_monitors(&triple(), None, "now");
        for s in &mut rig.screens {
            s.panel.bezel = Bezel {
                left: Mm(7.5),
                right: Mm(7.5),
                top: Mm(9.0),
                bottom: Mm(14.0),
            };
            s.mounting.angle = Deg(55.0);
            s.mounting.gap = Mm(3.0);
            s.mounting.vertical_offset = Mm(-12.0);
            s.panel.curvature = Curvature::Radius { radius: Mm(1000.0) };
        }

        let again = rig_from_monitors(&triple(), Some(&rig), "later");
        for s in &again.screens {
            assert_eq!(s.panel.bezel.bottom, Mm(14.0), "bezels survive");
            assert_eq!(s.mounting.angle, Deg(55.0), "angles survive");
            assert_eq!(s.mounting.gap, Mm(3.0), "mount gaps survive");
            assert_eq!(
                s.mounting.vertical_offset,
                Mm(-12.0),
                "vertical offsets survive"
            );
            assert!(
                matches!(s.panel.curvature, Curvature::Radius { .. }),
                "curvature survives"
            );
        }
    }

    #[test]
    fn measurements_survive_a_cable_swap() {
        // Two monitors change ports: same panels, different device paths and
        // different positions. Matching by EDID identity means the bezels stay
        // with the right screens.
        let mut rig = rig_from_monitors(&triple(), None, "now");
        rig.screens[0].panel.bezel.left = Mm(11.0);

        let mut swapped = triple();
        swapped[0].device_path = r"\\?\DISPLAY#TST0001#NEWPORT#{guid}".into();
        swapped[0].gdi_name = r"\\.\DISPLAY7".into();

        let again = rig_from_monitors(&swapped, Some(&rig), "later");
        let left = again
            .screens
            .iter()
            .find(|s| s.role == ScreenRole::Left)
            .unwrap();
        assert_eq!(
            left.panel.bezel.left,
            Mm(11.0),
            "the measurement followed the panel"
        );
    }

    #[test]
    fn a_manual_size_is_not_overwritten_by_edid() {
        // Someone who measured the glass and typed the number in did so because
        // they trusted it more than the panel's own claim.
        let mut rig = rig_from_monitors(&triple(), None, "now");
        let centre = rig
            .screens
            .iter_mut()
            .find(|s| s.role == ScreenRole::Center)
            .unwrap();
        centre.panel.visible_width = Measurement::manual(1188.0);

        let again = rig_from_monitors(&triple(), Some(&rig), "later");
        let centre = again.center().unwrap();
        assert_eq!(centre.panel.visible_width.mm, Mm(1188.0));
        assert_eq!(
            centre.panel.visible_width.source,
            crate::MeasurementSource::Manual
        );
    }

    #[test]
    fn the_widest_panel_is_the_centre_when_nothing_is_primary() {
        let mut monitors = triple();
        for m in &mut monitors {
            m.is_primary = false;
        }
        let rig = rig_from_monitors(&monitors, None, "now");
        assert_eq!(rig.center().unwrap().panel.native_resolution.width, 5120);
    }

    #[test]
    fn a_single_monitor_is_the_centre() {
        let rig = rig_from_monitors(
            &[monitor(0, 5120, "C", true, Some((1193.0, 336.0)))],
            None,
            "now",
        );
        assert_eq!(rig.screens.len(), 1);
        assert_eq!(rig.screens[0].role, ScreenRole::Center);
    }

    #[test]
    fn no_monitors_leaves_an_existing_rig_alone() {
        // A cable out is a temporary state, not a decision to erase the rig.
        let rig = rig_from_monitors(&triple(), None, "now");
        let after = rig_from_monitors(&[], Some(&rig), "later");
        assert_eq!(after.screens.len(), 3);
    }
}
