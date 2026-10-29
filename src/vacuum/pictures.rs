//! Generated pictures as SVG, ported from the extension's
//! `core/pictures.js`: a 2 x 2 cover of shapes, or eight rings for a
//! person. Every colour comes from the palette by index.
//!
//! Every picture has a thin light outline: palette black (#1A1A1A) is
//! almost the app's dark background and would otherwise vanish.

use super::derive::{PALETTE, Picture, Shape};

/// A shape in a 50 x 50 square, nudged off-centre so a quarter turn also
/// moves the symmetric ones.
fn shape_svg(shape: Shape, color: &str) -> String {
    match shape {
        Shape::Circle => format!(r#"<circle cx="21" cy="21" r="13" fill="{color}"/>"#),
        Shape::Square => format!(r#"<rect x="9" y="9" width="24" height="24" fill="{color}"/>"#),
        Shape::Triangle => format!(r#"<polygon points="25,7 42,38 8,38" fill="{color}"/>"#),
        Shape::Diamond => format!(r#"<polygon points="21,5 37,21 21,37 5,21" fill="{color}"/>"#),
        Shape::HalfDisc => format!(r#"<path d="M9,31 A16,16 0 0 1 41,31 Z" fill="{color}"/>"#),
        Shape::Ring => format!(
            r#"<circle cx="21" cy="21" r="11" fill="none" stroke="{color}" stroke-width="7"/>"#
        ),
        Shape::Bar => format!(r#"<rect x="7" y="13" width="36" height="11" fill="{color}"/>"#),
        Shape::Cross => {
            format!(r#"<path d="M17,7h9v10h10v9h-10v10h-9v-10h-10v-9h10z" fill="{color}"/>"#)
        }
    }
}

/// Relative ring widths, outermost first.
const RING_PROFILES: [[f64; 8]; 4] = [
    [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    [1.6, 1.4, 1.2, 1.0, 0.9, 0.8, 0.7, 0.8],
    [1.4, 0.7, 1.4, 0.7, 1.3, 0.7, 1.1, 0.9],
    [2.2, 1.0, 1.0, 0.8, 0.8, 0.7, 0.6, 1.0],
];

// The extension draws the outline in screen pixels whatever the size
// (`vector-effect="non-scaling-stroke"`). Pictures here are drawn at a
// fixed size and scaled as images, so the outline is a fixed share of it.
const OUTLINE: &str = r##"fill="none" stroke="#FFFFFF" stroke-opacity="0.4" stroke-width="1.5""##;

/// The picture as SVG, `size` pixels square.
pub fn svg(picture: &Picture, size: u32) -> String {
    match picture {
        Picture::Cover(squares) => {
            let cells: String = squares
                .iter()
                .enumerate()
                .map(|(index, square)| {
                    let (x, y) = ((index % 2) * 50, (index / 2) * 50);
                    format!(
                        r#"<g transform="translate({x} {y})"><rect width="50" height="50" fill="{}"/><g transform="rotate({} 25 25)">{}</g></g>"#,
                        PALETTE[usize::from(square.background)],
                        u32::from(square.turn) * 90,
                        shape_svg(square.shape, PALETTE[usize::from(square.color)])
                    )
                })
                .collect();
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="{size}" height="{size}">{cells}<rect x="0.5" y="0.5" width="99" height="99" {OUTLINE}/></svg>"#
            )
        }
        Picture::Rings { rings, profile } => {
            let weights = RING_PROFILES[usize::from(*profile) % RING_PROFILES.len()];
            let total: f64 = weights.iter().sum();
            let mut radius = 50.0;
            let circles: String = rings
                .iter()
                .zip(weights)
                .map(|(color, weight)| {
                    let circle = format!(
                        r#"<circle cx="50" cy="50" r="{radius:.2}" fill="{}"/>"#,
                        PALETTE[usize::from(*color)]
                    );
                    radius -= 50.0 * weight / total;
                    circle
                })
                .collect();
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="{size}" height="{size}">{circles}<circle cx="50" cy="50" r="49.5" {OUTLINE}/></svg>"#
            )
        }
    }
}
