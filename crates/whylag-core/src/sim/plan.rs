//! Simulated office layout and its floor plan drawing.

use super::{HEIGHT_M, MARGIN_M, PX_PER_M, WIDTH_M};
use crate::rf::{Material, Wall};
use serde::Serialize;

fn w(x1: f64, y1: f64, x2: f64, y2: f64, material: Material) -> Wall {
    Wall {
        x1,
        y1,
        x2,
        y2,
        material,
    }
}

/// Walls in metres. Door gaps are real gaps, so signal leaks through doorways.
pub fn walls() -> Vec<Wall> {
    use Material::*;
    vec![
        // Exterior
        w(0.0, 0.0, 42.0, 0.0, Concrete),
        w(42.0, 0.0, 42.0, 26.0, Concrete),
        w(42.0, 26.0, 0.0, 26.0, Concrete),
        w(0.0, 26.0, 0.0, 0.0, Concrete),
        // Corridor, north side (y = 11)
        w(0.0, 11.0, 8.0, 11.0, Drywall),
        w(9.5, 11.0, 18.0, 11.0, Drywall),
        w(18.0, 11.0, 20.0, 11.0, Glass),
        w(21.0, 11.0, 24.0, 11.0, Glass),
        w(24.0, 11.0, 26.0, 11.0, Glass),
        w(27.0, 11.0, 30.0, 11.0, Glass),
        w(30.0, 11.0, 31.0, 11.0, Drywall),
        w(32.0, 11.0, 42.0, 11.0, Drywall),
        // North rooms
        w(18.0, 0.0, 18.0, 11.0, Drywall),
        w(24.0, 0.0, 24.0, 11.0, Glass),
        w(30.0, 0.0, 30.0, 11.0, Drywall),
        // Corridor, south side (y = 15)
        w(0.0, 15.0, 8.0, 15.0, Drywall),
        w(9.5, 15.0, 20.0, 15.0, Drywall),
        w(23.0, 15.0, 24.0, 15.0, Drywall),
        w(25.5, 15.0, 27.0, 15.0, Drywall),
        // Lift shaft
        w(20.0, 15.0, 23.0, 15.0, Shaft),
        w(20.0, 15.0, 20.0, 19.0, Shaft),
        w(23.0, 15.0, 23.0, 19.0, Shaft),
        w(20.0, 19.0, 23.0, 19.0, Shaft),
        // Kitchen
        w(20.0, 19.0, 20.0, 26.0, Drywall),
        // Server room
        w(27.0, 15.0, 32.0, 15.0, Metal),
        w(27.0, 15.0, 27.0, 26.0, Concrete),
        w(32.0, 15.0, 32.0, 26.0, Concrete),
        // South-east office behind a fire-rated wall
        w(32.0, 15.0, 33.0, 15.0, Brick),
        w(34.0, 15.0, 42.0, 15.0, Brick),
    ]
}

struct Room {
    name: &'static str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

const ROOMS: &[Room] = &[
    Room {
        name: "Open Space A",
        x: 0.0,
        y: 0.0,
        w: 18.0,
        h: 11.0,
    },
    Room {
        name: "Meeting 1",
        x: 18.0,
        y: 0.0,
        w: 6.0,
        h: 11.0,
    },
    Room {
        name: "Meeting 2",
        x: 24.0,
        y: 0.0,
        w: 6.0,
        h: 11.0,
    },
    Room {
        name: "Office North",
        x: 30.0,
        y: 0.0,
        w: 12.0,
        h: 11.0,
    },
    Room {
        name: "Corridor",
        x: 0.0,
        y: 11.0,
        w: 42.0,
        h: 4.0,
    },
    Room {
        name: "Open Space B",
        x: 0.0,
        y: 15.0,
        w: 20.0,
        h: 11.0,
    },
    Room {
        name: "Lift",
        x: 20.0,
        y: 15.0,
        w: 3.0,
        h: 4.0,
    },
    Room {
        name: "Kitchen",
        x: 20.0,
        y: 19.0,
        w: 7.0,
        h: 7.0,
    },
    Room {
        name: "Server Room",
        x: 27.0,
        y: 15.0,
        w: 5.0,
        h: 11.0,
    },
    Room {
        name: "Office South-East",
        x: 32.0,
        y: 15.0,
        w: 10.0,
        h: 11.0,
    },
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloorPlan {
    pub name: String,
    pub image_data_url: String,
    pub width_px: u32,
    pub height_px: u32,
    pub meters_per_px: f64,
    /// Walls in floor-plan pixels, ready for the predictive planner.
    pub walls_px: Vec<Wall>,
}

fn wall_style(m: Material) -> (&'static str, f64) {
    match m {
        Material::Concrete => ("#3d4148", 7.0),
        Material::Brick => ("#8a4b3a", 6.0),
        Material::Drywall => ("#80868f", 3.5),
        Material::Glass => ("#6fb3d6", 3.0),
        Material::Wood => ("#a0773f", 3.5),
        Material::Metal => ("#4f5e7a", 6.0),
        Material::Shaft => ("#2b2e33", 7.0),
    }
}

fn px(m: f64) -> f64 {
    (m + MARGIN_M) * PX_PER_M
}

pub fn floor_plan(walls: &[Wall]) -> FloorPlan {
    let width_px = ((WIDTH_M + 2.0 * MARGIN_M) * PX_PER_M) as u32;
    let height_px = ((HEIGHT_M + 2.0 * MARGIN_M) * PX_PER_M) as u32;
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width_px}" height="{height_px}" viewBox="0 0 {width_px} {height_px}"><rect width="100%" height="100%" fill="#fbfbf8"/><defs><pattern id="h" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><line x1="0" y1="0" x2="0" y2="6" stroke="#c9cbcf" stroke-width="2"/></pattern></defs>"##
    );
    for room in ROOMS {
        let fill = match room.name {
            "Corridor" => "#f1f1ec",
            "Lift" => "url(#h)",
            "Server Room" => "#eceef2",
            _ => "#ffffff",
        };
        svg.push_str(&format!(
            r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{fill}"/>"##,
            px(room.x),
            px(room.y),
            room.w * PX_PER_M,
            room.h * PX_PER_M
        ));
    }
    // Desks in the open spaces make the plan read as an office at a glance.
    for (ox, oy, cols, rows) in [(2.0, 2.0, 4, 3), (2.0, 17.5, 4, 3)] {
        for c in 0..cols {
            for r in 0..rows {
                svg.push_str(&format!(
                    r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="2" fill="none" stroke="#d5d7da" stroke-width="1.5"/>"##,
                    px(ox + c as f64 * 3.6),
                    px(oy + r as f64 * 2.6),
                    2.8 * PX_PER_M,
                    1.4 * PX_PER_M
                ));
            }
        }
    }
    for wall in walls {
        let (color, width) = wall_style(wall.material);
        svg.push_str(&format!(
            r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{color}" stroke-width="{width}" stroke-linecap="square"/>"##,
            px(wall.x1),
            px(wall.y1),
            px(wall.x2),
            px(wall.y2)
        ));
    }
    for room in ROOMS.iter().filter(|r| r.name != "Lift") {
        svg.push_str(&format!(
            r##"<text x="{:.1}" y="{:.1}" font-family="Helvetica, Arial, sans-serif" font-size="12" fill="#9a9ea6" text-anchor="middle" letter-spacing="0.5">{}</text>"##,
            px(room.x + room.w / 2.0),
            px(room.y + room.h / 2.0) + 4.0,
            room.name.to_uppercase()
        ));
    }
    svg.push_str("</svg>");
    let walls_px = walls
        .iter()
        .map(|w| Wall {
            x1: px(w.x1),
            y1: px(w.y1),
            x2: px(w.x2),
            y2: px(w.y2),
            material: w.material,
        })
        .collect();
    FloorPlan {
        name: "Demo office".into(),
        image_data_url: format!(
            "data:image/svg+xml;base64,{}",
            crate::util::base64(svg.as_bytes())
        ),
        width_px,
        height_px,
        meters_per_px: 1.0 / PX_PER_M,
        walls_px,
    }
}
