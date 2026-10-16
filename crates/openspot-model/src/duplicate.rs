//! Deep copies of zones and snapshots with fresh identities.

use crate::{Snapshot, Uuid, Zone};

/// Suffix appended to the name of a duplicate.
const COPY_SUFFIX: &str = " copy";

impl Snapshot {
    /// Deep copy with new ids for the snapshot and every scan point, named "<name> copy".
    pub fn duplicate(&self) -> Self {
        let mut copy = self.clone();
        copy.id = Uuid::new_v4();
        copy.name = format!("{}{COPY_SUFFIX}", self.name);
        for point in &mut copy.points {
            point.id = Uuid::new_v4();
        }
        copy
    }
}

impl Zone {
    /// Deep copy (floor plan, scale, all snapshots and points) with new ids throughout,
    /// named "<name> copy". Copied ids must differ: undo history and map textures are keyed by them.
    pub fn duplicate(&self) -> Self {
        let mut copy = self.clone();
        copy.id = Uuid::new_v4();
        copy.name = format!("{}{COPY_SUFFIX}", self.name);
        copy.snapshots = self.snapshots.iter().map(|s| {
            let mut dup = s.duplicate();
            dup.name = s.name.clone();
            dup
        }).collect();
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapImage, ScanPoint, Scale};

    fn sample_zone() -> Zone {
        let mut z = Zone::new("Lobby");
        z.map = Some(MapImage { original_name: "p.png".into(), data: vec![1, 2, 3], width: 4, height: 5 });
        z.scale = Scale::new(12.5);
        z.snapshots[0].points.push(ScanPoint::new(1.0, 2.0, 7, vec![]));
        z.snapshots[0].points.push(ScanPoint::new(3.0, 4.0, 8, vec![]));
        z
    }

    #[test]
    fn zone_copy_has_fresh_ids_and_equal_data() {
        let z = sample_zone();
        let c = z.duplicate();
        assert_ne!(c.id, z.id);
        assert_eq!(c.name, "Lobby copy");
        assert_eq!((&c.map, c.scale), (&z.map, z.scale));
        assert_eq!(c.snapshots.len(), z.snapshots.len());
        let (s, cs) = (&z.snapshots[0], &c.snapshots[0]);
        assert_ne!(cs.id, s.id);
        assert_eq!(cs.name, s.name);
        for (p, cp) in s.points.iter().zip(&cs.points) {
            assert_ne!(p.id, cp.id);
            assert_eq!((p.x, p.y, p.time), (cp.x, cp.y, cp.time));
        }
    }

    #[test]
    fn snapshot_copy_is_named_and_independent() {
        let mut s = Snapshot::new("Day");
        s.points.push(ScanPoint::new(1.0, 1.0, 1, vec![]));
        let mut c = s.duplicate();
        assert_eq!(c.name, "Day copy");
        assert_ne!(c.id, s.id);
        assert_ne!(c.points[0].id, s.points[0].id);
        c.points.clear();
        assert_eq!(s.points.len(), 1);
    }
}
