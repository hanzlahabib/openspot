//! Material choices and custom-material editing rules (pure; the egui editor is in `ui/`).
//!
//! Presets come from the shipped catalog and are read-only. A user changes a preset by
//! copying it into the zone's custom materials, which shadow nothing: custom ids always
//! carry a `custom-` prefix, so a catalog update can never silently change a user's wall.

use openspot_model::{Band, Material, MaterialCatalog, MaterialCategory, PerBand, PlanningData, Project, Uuid, WallKind};

use super::edit::{Change, Edit};

/// Largest loss a custom material may declare, per crossing. A 1 m thick reinforced vault
/// is well under this; beyond it a typed value is almost certainly a mistake.
pub const MAX_ABSORPTION_DB: f64 = 100.0;

pub const CUSTOM_ID_PREFIX: &str = "custom-";

/// What a material-editing action can be refused for.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MaterialError {
    #[error("give the material a name")]
    EmptyName,
    #[error("a custom material called '{0}' already exists on this level")]
    DuplicateName(String),
    #[error("loss must be between 0 and {MAX_ABSORPTION_DB} dB and reflection between 0 and 1")]
    InvalidValue,
    #[error("'{0}' is used by {1} wall, opening or slab setting(s); change those first")]
    InUse(String, usize),
    #[error("no such custom material")]
    NotFound,
}

/// The editable fields of a custom material.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialDraft {
    pub name: String,
    pub category: MaterialCategory,
    pub absorption_db: PerBand,
    pub reflection: PerBand,
}

impl MaterialDraft {
    /// A starting point for a new material: a modest wall.
    pub fn new_wall() -> Self {
        Self { name: String::new(), category: MaterialCategory::Wall, absorption_db: PerBand::splat(3.0), reflection: PerBand::splat(0.3) }
    }

    /// A copy of `m` to edit as a new custom material.
    pub fn copy_of(m: &Material) -> Self {
        Self { name: format!("{} (custom)", m.name), category: m.category, absorption_db: m.absorption_db, reflection: m.reflection }
    }

    pub fn from_material(m: &Material) -> Self {
        Self { name: m.name.clone(), category: m.category, absorption_db: m.absorption_db, reflection: m.reflection }
    }

    fn validate(&self) -> Result<String, MaterialError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(MaterialError::EmptyName);
        }
        let ok = Band::ALL.iter().all(|&b| {
            let (a, r) = (self.absorption_db.get(b), self.reflection.get(b));
            a.is_finite() && (0.0..=MAX_ABSORPTION_DB).contains(&a) && r.is_finite() && (0.0..=1.0).contains(&r)
        });
        if ok { Ok(name.to_owned()) } else { Err(MaterialError::InvalidValue) }
    }
}

/// Lowercase ASCII slug of `name` for ids; non-alphanumerics collapse to single dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn id_taken(id: &str, data: &PlanningData, catalog: &MaterialCatalog) -> bool {
    data.materials.iter().any(|m| m.id == id) || catalog.get(id).is_some()
}

/// A fresh id derived from `name` that clashes with no custom material or preset.
pub fn unique_id(name: &str, data: &PlanningData, catalog: &MaterialCatalog) -> String {
    let base = format!("{CUSTOM_ID_PREFIX}{}", match slug(name) {
        s if s.is_empty() => "material".to_owned(),
        s => s,
    });
    if !id_taken(&base, data, catalog) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|id| !id_taken(id, data, catalog)).unwrap_or(base)
}

fn name_clash(data: &PlanningData, name: &str, except: Option<&str>) -> bool {
    data.materials.iter().any(|m| Some(m.id.as_str()) != except && m.name.eq_ignore_ascii_case(name))
}

fn build(id: String, name: String, draft: &MaterialDraft) -> Material {
    let mut m = Material::custom(id, name, draft.category, 0.0, 0.0);
    m.absorption_db = draft.absorption_db;
    m.reflection = draft.reflection;
    m
}

/// Edit that adds a new custom material to `zone`.
pub fn add_edit(zone: Uuid, data: &PlanningData, catalog: &MaterialCatalog, draft: &MaterialDraft) -> Result<(Edit, String), MaterialError> {
    let name = draft.validate()?;
    if name_clash(data, &name, None) {
        return Err(MaterialError::DuplicateName(name));
    }
    let id = unique_id(&name, data, catalog);
    let material = build(id.clone(), name, draft);
    Ok((Edit::Material { zone, change: Change::added(data.materials.len(), material) }, id))
}

/// Edit that rewrites custom material `id` (its id stays, so walls keep using it).
pub fn update_edit(zone: Uuid, data: &PlanningData, id: &str, draft: &MaterialDraft) -> Result<Edit, MaterialError> {
    let name = draft.validate()?;
    let index = data.materials.iter().position(|m| m.id == id).ok_or(MaterialError::NotFound)?;
    if name_clash(data, &name, Some(id)) {
        return Err(MaterialError::DuplicateName(name));
    }
    let before = data.materials[index].clone();
    let mut after = build(id.to_owned(), name, draft);
    after.source = before.source.clone();
    Ok(Edit::Material { zone, change: Change::modified(index, before, after) })
}

/// How many walls, openings and the slab refer to material `id`.
pub fn usage_count(data: &PlanningData, id: &str) -> usize {
    let walls = data.walls.iter().map(|w| usize::from(w.material == id) + w.openings.iter().filter(|o| o.material == id).count());
    walls.sum::<usize>() + usize::from(data.level.slab.material == id)
}

/// Edit that deletes custom material `id`, refused while anything still uses it.
pub fn remove_edit(zone: Uuid, data: &PlanningData, id: &str) -> Result<Edit, MaterialError> {
    let index = data.materials.iter().position(|m| m.id == id).ok_or(MaterialError::NotFound)?;
    match usage_count(data, id) {
        0 => Ok(Edit::Material { zone, change: Change::removed(index, data.materials[index].clone()) }),
        n => Err(MaterialError::InUse(data.materials[index].name.clone(), n)),
    }
}

/// Edit that copies custom material `id` of `from` into the other zones of `zones` that do
/// not have it yet (custom materials are per zone, so each level using one needs its own
/// copy). `None` when there is nothing to copy.
pub fn copy_to_zones_edit(project: &Project, from: Uuid, id: &str, zones: &[Uuid]) -> Option<Edit> {
    let material = project.zones.iter().find(|z| z.id == from)?.planning.materials.iter().find(|m| m.id == id)?.clone();
    let edits: Vec<Edit> = zones
        .iter()
        .filter(|&&z| z != from)
        .filter_map(|&z| project.zones.iter().find(|zone| zone.id == z))
        .filter(|zone| !zone.planning.materials.iter().any(|m| m.id == id))
        .map(|zone| Edit::Material { zone: zone.id, change: Change::added(zone.planning.materials.len(), material.clone()) })
        .collect();
    (!edits.is_empty()).then_some(Edit::Batch(edits))
}

/// The category whose materials fit a wall of `kind`.
pub fn category_for(kind: WallKind) -> MaterialCategory {
    match kind {
        WallKind::Wall => MaterialCategory::Wall,
        WallKind::Door => MaterialCategory::Door,
        WallKind::Window => MaterialCategory::Window,
        WallKind::Shelf => MaterialCategory::Shelf,
    }
}

/// One entry of a material picker.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub custom: bool,
}

/// Materials of `category`: the zone's custom ones first, then the presets.
pub fn choices(data: &PlanningData, catalog: &MaterialCatalog, category: MaterialCategory) -> Vec<Choice> {
    let custom = data.materials.iter().filter(|m| m.category == category).map(|m| Choice { id: m.id.clone(), label: format!("{} (custom)", m.name), custom: true });
    let presets = catalog.by_category(category).map(|m| Choice { id: m.id.clone(), label: m.name.clone(), custom: false });
    custom.chain(presets).collect()
}

/// Display name of material `id`, falling back to the id for one that no longer exists.
pub fn display_name(data: &PlanningData, catalog: &MaterialCatalog, id: &str) -> String {
    data.resolve_material(id, catalog).map_or_else(|| format!("{id} (missing)"), |m| m.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::{Opening, OpeningKind, Point, Wall, Zone};

    fn catalog() -> MaterialCatalog {
        MaterialCatalog::builtin().unwrap()
    }

    fn draft(name: &str, db: f64) -> MaterialDraft {
        MaterialDraft { name: name.into(), category: MaterialCategory::Wall, absorption_db: PerBand { ghz2_4: db, ghz5: db * 2.0, ghz6: db * 3.0 }, reflection: PerBand::splat(0.4) }
    }

    fn apply(project: &mut Project, e: &Edit) {
        assert!(e.apply(project, true));
    }

    #[test]
    fn slug_and_unique_ids() {
        assert_eq!(slug("  Lead-lined  wall (10 mm)! "), "lead-lined-wall-10-mm");
        assert_eq!(slug("!!!"), "");
        let mut data = PlanningData::default();
        let cat = catalog();
        assert_eq!(unique_id("Foam board", &data, &cat), "custom-foam-board");
        data.materials.push(Material::custom("custom-foam-board", "Foam board", MaterialCategory::Wall, 1.0, 0.1));
        assert_eq!(unique_id("Foam board", &data, &cat), "custom-foam-board-2");
        assert_eq!(unique_id("???", &data, &cat), "custom-material");
    }

    #[test]
    fn adding_stores_per_band_values_exactly() {
        let mut project = Project::new("p");
        let zone = project.zones[0].id;
        let (edit, id) = add_edit(zone, &project.zones[0].planning, &catalog(), &draft("Foam board", 2.0)).unwrap();
        apply(&mut project, &edit);
        let m = project.zones[0].planning.resolve_material(&id, &catalog()).unwrap().clone();
        assert_eq!((m.absorption_db.ghz2_4, m.absorption_db.ghz5, m.absorption_db.ghz6), (2.0, 4.0, 6.0));
        assert_eq!(m.reflection.ghz5, 0.4);
        assert_eq!((m.name.as_str(), m.source.as_str()), ("Foam board", "custom"));
        assert!(m.is_valid());
    }

    #[test]
    fn invalid_drafts_are_refused() {
        let data = PlanningData::default();
        let zone = Uuid::new_v4();
        let err = |d: MaterialDraft| add_edit(zone, &data, &catalog(), &d).unwrap_err();
        assert_eq!(err(draft("  ", 1.0)), MaterialError::EmptyName);
        assert_eq!(err(draft("x", -1.0)), MaterialError::InvalidValue);
        assert_eq!(err(draft("x", MAX_ABSORPTION_DB)), MaterialError::InvalidValue, "the 3x 6 GHz value exceeds the limit");
        assert_eq!(err(draft("x", f64::NAN)), MaterialError::InvalidValue);
        let mut d = draft("x", 1.0);
        d.reflection.ghz5 = 1.5;
        assert_eq!(err(d), MaterialError::InvalidValue);
    }

    #[test]
    fn names_are_unique_per_zone_ignoring_case() {
        let mut project = Project::new("p");
        let zone = project.zones[0].id;
        let (e, id) = add_edit(zone, &project.zones[0].planning, &catalog(), &draft("Foam", 1.0)).unwrap();
        apply(&mut project, &e);
        let data = &project.zones[0].planning;
        assert_eq!(add_edit(zone, data, &catalog(), &draft("FOAM", 1.0)).unwrap_err(), MaterialError::DuplicateName("FOAM".into()));
        // Renaming a material to its own name is fine.
        assert!(update_edit(zone, data, &id, &draft("Foam", 5.0)).is_ok());
    }

    #[test]
    fn update_keeps_the_id_so_walls_follow_the_new_values() {
        let mut project = Project::new("p");
        let zone = project.zones[0].id;
        let (e, id) = add_edit(zone, &project.zones[0].planning, &catalog(), &draft("Foam", 1.0)).unwrap();
        apply(&mut project, &e);
        project.zones[0].planning.walls.push(Wall::new(Point::new(0.0, 0.0), Point::new(1.0, 0.0), openspot_model::WallKind::Wall, id.clone()));
        let e = update_edit(zone, &project.zones[0].planning, &id, &draft("Foam v2", 9.0)).unwrap();
        apply(&mut project, &e);
        let cat = catalog();
        let used = project.zones[0].planning.walls[0].material.clone();
        let m = project.zones[0].planning.resolve_material(&used, &cat).unwrap();
        assert_eq!((m.name.as_str(), m.absorption_db.ghz2_4), ("Foam v2", 9.0));
        assert!(e.apply(&mut project, false));
        assert_eq!(project.zones[0].planning.materials[0].name, "Foam");
        assert_eq!(update_edit(zone, &project.zones[0].planning, "custom-nope", &draft("a", 1.0)).unwrap_err(), MaterialError::NotFound);
    }

    #[test]
    fn deleting_is_blocked_while_walls_openings_or_the_slab_use_it() {
        let mut project = Project::new("p");
        let zone = project.zones[0].id;
        let (e, id) = add_edit(zone, &project.zones[0].planning, &catalog(), &draft("Foam", 1.0)).unwrap();
        apply(&mut project, &e);
        assert!(remove_edit(zone, &project.zones[0].planning, &id).is_ok());

        let mut wall = Wall::new(Point::new(0.0, 0.0), Point::new(4.0, 0.0), openspot_model::WallKind::Wall, "drywall");
        wall.openings.push(Opening { id: Uuid::new_v4(), kind: OpeningKind::Door, start: 0.1, end: 0.3, material: id.clone() });
        project.zones[0].planning.walls.push(wall);
        assert_eq!(remove_edit(zone, &project.zones[0].planning, &id).unwrap_err(), MaterialError::InUse("Foam".into(), 1));
        project.zones[0].planning.level.slab.material = id.clone();
        assert_eq!(usage_count(&project.zones[0].planning, &id), 2);
        project.zones[0].planning.walls.clear();
        project.zones[0].planning.level.slab.material = "concrete-slab".into();
        let e = remove_edit(zone, &project.zones[0].planning, &id).unwrap();
        apply(&mut project, &e);
        assert!(project.zones[0].planning.materials.is_empty());
    }

    #[test]
    fn copying_to_other_levels_skips_zones_that_already_have_it() {
        let mut project = Project::new("p");
        project.zones.push(Zone::new("two"));
        project.zones.push(Zone::new("three"));
        let ids: Vec<Uuid> = project.zones.iter().map(|z| z.id).collect();
        let (e, id) = add_edit(ids[0], &project.zones[0].planning, &catalog(), &draft("Foam", 1.0)).unwrap();
        apply(&mut project, &e);
        let existing = project.zones[0].planning.materials[0].clone();
        project.zones[2].planning.materials.push(existing);
        let copy = copy_to_zones_edit(&project, ids[0], &id, &ids).unwrap();
        apply(&mut project, &copy);
        assert_eq!(project.zones.iter().map(|z| z.planning.materials.len()).collect::<Vec<_>>(), vec![1, 1, 1]);
        assert!(copy_to_zones_edit(&project, ids[0], &id, &ids).is_none(), "nothing left to copy");
        assert!(copy_to_zones_edit(&project, ids[0], "custom-nope", &ids).is_none());
    }

    #[test]
    fn pickers_list_custom_first_and_only_the_right_category() {
        let mut data = PlanningData::default();
        data.materials.push(Material::custom("custom-a", "Aaa", MaterialCategory::Door, 1.0, 0.1));
        data.materials.push(Material::custom("custom-b", "Bbb", MaterialCategory::Wall, 1.0, 0.1));
        let doors = choices(&data, &catalog(), MaterialCategory::Door);
        assert_eq!(doors[0], Choice { id: "custom-a".into(), label: "Aaa (custom)".into(), custom: true });
        assert!(doors.iter().skip(1).all(|c| !c.custom) && doors.iter().any(|c| c.id == "wood-door"));
        assert!(!doors.iter().any(|c| c.id == "custom-b" || c.id == "drywall"));
        assert_eq!(category_for(WallKind::Shelf), MaterialCategory::Shelf);
        assert_eq!(display_name(&data, &catalog(), "custom-b"), "Bbb");
        assert_eq!(display_name(&data, &catalog(), "gone"), "gone (missing)");
    }

    #[test]
    fn draft_helpers_start_from_existing_materials() {
        let cat = catalog();
        let d = MaterialDraft::copy_of(cat.get("drywall").unwrap());
        assert!(d.name.ends_with("(custom)"));
        assert_eq!(d.absorption_db, cat.get("drywall").unwrap().absorption_db);
        assert_eq!(MaterialDraft::from_material(cat.get("drywall").unwrap()).name, "Plasterboard partition (2.5 cm)");
    }
}
