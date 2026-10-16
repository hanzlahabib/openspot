//! Persistence of drawn plans (vector shapes per zone).

use rusqlite::{Connection, Transaction, params};

use openspot_model::{Drawing, FontSize, LineThickness, Project, Shape, ShapeKind};

use crate::{StoreError, parse_uuid};

/// `canvas_shape.kind` codes; stored values, never renumber.
const KIND_POLYLINE: i64 = 0;
const KIND_RECT: i64 = 1;
const KIND_TEXT: i64 = 2;

pub(super) fn save(tx: &Transaction<'_>, project: &Project) -> Result<(), StoreError> {
    for (id, zone) in project.canvas.zones() {
        let Some(drawing) = &zone.drawing else { continue };
        let zid = id.to_string();
        tx.execute("INSERT INTO canvas_drawing (zone_id, width, height) VALUES (?1, ?2, ?3)", params![zid, drawing.width, drawing.height])?;
        for (pos, shape) in drawing.shapes.iter().enumerate() {
            let (kind, text, filled, vertices): (i64, Option<&str>, bool, Vec<(f64, f64)>) = match &shape.kind {
                ShapeKind::Polyline { points } => (KIND_POLYLINE, None, false, points.clone()),
                ShapeKind::Rect { min, max, filled } => (KIND_RECT, None, *filled, vec![*min, *max]),
                ShapeKind::Text { at, text } => (KIND_TEXT, Some(text.as_str()), false, vec![*at]),
            };
            tx.execute(
                "INSERT INTO canvas_shape (zone_id, position, id, kind, thickness, font, text, filled) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![zid, pos, shape.id.to_string(), kind, shape.thickness.code(), shape.font.code(), text, filled],
            )?;
            for (vi, (x, y)) in vertices.iter().enumerate() {
                tx.execute(
                    "INSERT INTO canvas_shape_vertex (zone_id, shape_position, position, x, y) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![zid, pos, vi, x, y],
                )?;
            }
        }
    }
    Ok(())
}

/// Loads drawings into `project.canvas`. Call after the zone rows of `canvas_zone` were
/// loaded, so the drawing joins the zone's other settings.
pub(super) fn load(conn: &Connection, project: &mut Project) -> Result<(), StoreError> {
    let mut drawings = conn.prepare("SELECT zone_id, width, height FROM canvas_drawing")?;
    let rows = drawings
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, u32>(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut shapes = conn.prepare("SELECT position, id, kind, thickness, font, text, filled FROM canvas_shape WHERE zone_id = ?1 ORDER BY position")?;
    let mut vertices = conn.prepare("SELECT x, y FROM canvas_shape_vertex WHERE zone_id = ?1 AND shape_position = ?2 ORDER BY position")?;
    for (zid, width, height) in rows {
        let shape_rows = shapes
            .query_map([&zid], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, Option<String>>(5)?, r.get::<_, bool>(6)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut drawing = Drawing::new(width, height);
        for (pos, sid, kind, thickness, font, text, filled) in shape_rows {
            let pts = vertices.query_map(params![zid, pos], |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
            let kind = match (kind, pts.as_slice()) {
                (KIND_POLYLINE, _) if pts.len() >= 2 => ShapeKind::Polyline { points: pts },
                (KIND_RECT, [a, b]) => ShapeKind::Rect { min: *a, max: *b, filled },
                (KIND_TEXT, [at]) => ShapeKind::Text { at: *at, text: text.unwrap_or_default() },
                _ => return Err(StoreError::Corrupt("drawing shape is malformed")),
            };
            drawing.shapes.push(Shape {
                id: parse_uuid(&sid)?,
                kind,
                thickness: LineThickness::from_code(thickness).unwrap_or_default(),
                font: FontSize::from_code(font).unwrap_or_default(),
            });
        }
        project.canvas.zone_mut(parse_uuid(&zid)?).drawing = Some(drawing);
    }
    Ok(())
}
