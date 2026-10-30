//! Workspace aids that don't change the network: freehand and polygon drawings, and clusters
//! (several devices shown as one icon, opened to see what's inside).

use std::collections::{BTreeMap, BTreeSet};

use netlabcraft_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::{MAX_NAME, ModelError, Result, Shape, Topology, finite, parse_color};

/// Most points a freehand stroke keeps (after simplification), and a polygon's corners.
pub const MAX_FREEHAND_POINTS: usize = 512;
pub const MAX_POLYGON_POINTS: usize = 64;
/// Most points read from a request before simplifying (hostile input).
pub const MAX_RAW_POINTS: usize = 4_096;
/// How far (workspace units) simplification may move a freehand stroke.
pub const FREEHAND_TOLERANCE: f32 = 1.5;
/// Most clusters in a lab.
pub const MAX_CLUSTERS: usize = 500;

/// Devices grouped under one icon. Only the view changes: the devices, their cables and the
/// simulation are the same as without it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cluster {
    pub id: u32,
    pub name: String,
    /// Where its icon sits, in the view that shows it.
    pub x: f32,
    pub y: f32,
    /// The devices directly inside (a device is in at most one cluster).
    #[serde(default)]
    pub devices: Vec<DeviceId>,
    /// The cluster it sits in (none: the top level).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
}

/// How a device appears in a view: itself, or the cluster icon that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    Device,
    Cluster(u32),
}

fn finite_point(p: [f32; 2]) -> [f32; 2] {
    [finite(p[0]), finite(p[1])]
}

fn distance_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
    let len2 = abx * abx + aby * aby;
    let (apx, apy) = (p[0] - a[0], p[1] - a[1]);
    if len2 <= f32::EPSILON {
        return (apx * apx + apy * apy).sqrt();
    }
    let t = ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0);
    let (dx, dy) = (apx - abx * t, apy - aby * t);
    (dx * dx + dy * dy).sqrt()
}

/// Simplifies a path (Ramer–Douglas–Peucker, without recursion): drops points that sit within
/// `tolerance` of the line through their neighbours. Ends are kept.
pub fn simplify(points: &[[f32; 2]], tolerance: f32) -> Vec<[f32; 2]> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut stack = vec![(0usize, n - 1)];
    while let Some((start, end)) = stack.pop() {
        let (Some(&a), Some(&b)) = (points.get(start), points.get(end)) else { continue };
        let mut best = (0.0f32, None);
        for i in start + 1..end {
            if let Some(&p) = points.get(i) {
                let d = distance_to_segment(p, a, b);
                if d > best.0 {
                    best = (d, Some(i));
                }
            }
        }
        if let (d, Some(i)) = best
            && d > tolerance
        {
            if let Some(k) = keep.get_mut(i) {
                *k = true;
            }
            stack.push((start, i));
            stack.push((i, end));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Thins a path to at most `max` points, evenly, keeping both ends.
fn thin(points: Vec<[f32; 2]>, max: usize) -> Vec<[f32; 2]> {
    if points.len() <= max || max < 2 {
        return points.into_iter().take(max).collect();
    }
    let n = points.len();
    let step = (n - 1) as f32 / (max - 1) as f32;
    (0..max).filter_map(|i| points.get(((i as f32 * step).round() as usize).min(n - 1)).copied()).collect()
}

/// Cleans the points of a new freehand stroke or polygon (workspace coordinates): drops
/// non-finite and repeated points, simplifies a stroke, checks the count. Returns the top-left
/// corner and the points relative to it.
pub fn shape_points(shape: Shape, raw: &[[f32; 2]]) -> Result<(f32, f32, Vec<[f32; 2]>)> {
    let mut pts: Vec<[f32; 2]> = Vec::new();
    for p in raw.iter().take(MAX_RAW_POINTS) {
        if !(p[0].is_finite() && p[1].is_finite()) {
            continue;
        }
        let p = finite_point(*p);
        if pts.last() != Some(&p) {
            pts.push(p);
        }
    }
    let pts = match shape {
        Shape::Freehand => {
            let s = thin(simplify(&pts, FREEHAND_TOLERANCE), MAX_FREEHAND_POINTS);
            if s.len() < 2 {
                return Err(ModelError::Invalid("a freehand drawing needs at least two different points".into()));
            }
            s
        }
        Shape::Polygon => {
            // Closing back on the first corner is implied.
            if pts.len() > 1 && pts.first() == pts.last() {
                pts.pop();
            }
            if pts.len() < 3 {
                return Err(ModelError::Invalid("a polygon needs at least three corners".into()));
            }
            if pts.len() > MAX_POLYGON_POINTS {
                return Err(ModelError::Invalid(format!("a polygon has at most {MAX_POLYGON_POINTS} corners")));
            }
            pts
        }
        _ => return Err(ModelError::Invalid("only freehand drawings and polygons take points".into())),
    };
    let x = pts.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let y = pts.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let (x, y) = (finite(x), finite(y));
    Ok((x, y, pts.into_iter().map(|p| [p[0] - x, p[1] - y]).collect()))
}

/// A cluster name: trimmed, printable, at most [`MAX_NAME`] characters (`None` if empty).
fn clean_name(name: &str) -> Option<String> {
    let n: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    let n = n.trim().to_string();
    (!n.is_empty()).then_some(n)
}

impl Topology {
    /// Adds a freehand stroke or polygon through `points` (workspace coordinates); returns its id.
    pub fn add_path(&mut self, shape: Shape, points: &[[f32; 2]], color: Option<&str>, cluster: Option<u32>) -> Result<u32> {
        let (x, y, pts) = shape_points(shape, points)?;
        let id = self.add_annotation(x, y, "")?;
        let color = color.filter(|c| parse_color(c).is_some()).map(str::to_string);
        let w = pts.iter().map(|p| p[0]).fold(0.0, f32::max);
        let h = pts.iter().map(|p| p[1]).fold(0.0, f32::max);
        if let Ok(a) = self.annotation_mut(id) {
            a.shape = shape;
            a.points = pts;
            a.w = finite(w);
            a.h = finite(h);
            a.color = color;
            a.cluster = cluster;
        }
        Ok(id)
    }

    pub fn cluster(&self, id: u32) -> Option<&Cluster> {
        self.clusters.iter().find(|c| c.id == id)
    }

    pub fn cluster_mut(&mut self, id: u32) -> Result<&mut Cluster> {
        self.clusters.iter_mut().find(|c| c.id == id).ok_or_else(|| ModelError::Invalid(format!("no cluster {id}")))
    }

    /// The cluster a device is directly in.
    pub fn cluster_of(&self, dev: DeviceId) -> Option<u32> {
        self.clusters.iter().find(|c| c.devices.contains(&dev)).map(|c| c.id)
    }

    /// A cluster and its parents, innermost first (bounded, so a broken chain can't loop).
    fn chain(&self, start: Option<u32>) -> Vec<u32> {
        let mut out = Vec::new();
        let mut at = start;
        while let Some(c) = at {
            if out.contains(&c) || out.len() > self.clusters.len() {
                break;
            }
            out.push(c);
            at = self.cluster(c).and_then(|x| x.parent);
        }
        out
    }

    /// From the top level down to `cluster` (for a breadcrumb).
    pub fn cluster_path(&self, cluster: Option<u32>) -> Vec<u32> {
        let mut v = self.chain(cluster);
        v.reverse();
        v
    }

    /// What stands for something that lives in cluster `home` when looking at `view`: itself,
    /// the cluster icon of `view`'s child holding it, or nothing (it's elsewhere).
    fn lift(&self, home: Option<u32>, view: Option<u32>) -> Option<Shown> {
        if home == view {
            return Some(Shown::Device);
        }
        let chain = self.chain(home);
        let i = match view {
            None => chain.len().checked_sub(1)?,
            Some(v) => chain.iter().position(|c| *c == v)?.checked_sub(1)?,
        };
        chain.get(i).map(|c| Shown::Cluster(*c))
    }

    /// How a device appears in the view of cluster `view` (none: the top level).
    pub fn shown_as(&self, dev: DeviceId, view: Option<u32>) -> Option<Shown> {
        self.lift(self.cluster_of(dev), view)
    }

    /// [`Topology::shown_as`] for every device at once.
    pub fn shown_map(&self, view: Option<u32>) -> BTreeMap<DeviceId, Shown> {
        let mut home: BTreeMap<DeviceId, u32> = BTreeMap::new();
        for c in &self.clusters {
            for d in &c.devices {
                home.entry(*d).or_insert(c.id);
            }
        }
        let mut lifted: BTreeMap<Option<u32>, Option<Shown>> = BTreeMap::new();
        self.devices
            .iter()
            .filter_map(|d| {
                let h = home.get(&d.id).copied();
                let s = *lifted.entry(h).or_insert_with(|| self.lift(h, view));
                s.map(|s| (d.id, s))
            })
            .collect()
    }

    /// The clusters whose icons a view shows.
    pub fn clusters_in(&self, view: Option<u32>) -> impl Iterator<Item = &Cluster> {
        self.clusters.iter().filter(move |c| c.parent == view)
    }

    /// Groups devices and clusters that sit in the same view into a new cluster there; returns
    /// its id.
    pub fn create_cluster(&mut self, devices: &[DeviceId], clusters: &[u32], name: Option<&str>) -> Result<u32> {
        if self.clusters.len() >= MAX_CLUSTERS {
            return Err(ModelError::Invalid(format!("at most {MAX_CLUSTERS} clusters")));
        }
        let devices: Vec<DeviceId> = devices.iter().copied().collect::<BTreeSet<_>>().into_iter().collect();
        let clusters: Vec<u32> = clusters.iter().copied().collect::<BTreeSet<_>>().into_iter().collect();
        if devices.is_empty() && clusters.is_empty() {
            return Err(ModelError::Invalid("choose the devices to group".into()));
        }
        let mut level: Option<Option<u32>> = None;
        let mut sum = (0.0f32, 0.0f32);
        for d in &devices {
            let dev = self.device(*d).ok_or_else(|| ModelError::NoDevice(d.to_string()))?;
            sum = (sum.0 + dev.x, sum.1 + dev.y);
            let at = self.cluster_of(*d);
            if level.is_some_and(|l| l != at) {
                return Err(ModelError::Invalid("the devices to group must be in the same view".into()));
            }
            level = Some(at);
        }
        for c in &clusters {
            let cl = self.cluster(*c).ok_or_else(|| ModelError::Invalid(format!("no cluster {c}")))?;
            sum = (sum.0 + cl.x, sum.1 + cl.y);
            if level.is_some_and(|l| l != cl.parent) {
                return Err(ModelError::Invalid("the devices to group must be in the same view".into()));
            }
            level = Some(cl.parent);
        }
        let parent = level.flatten();
        let n = (devices.len() + clusters.len()) as f32;
        let id = self.clusters.iter().map(|c| c.id).max().unwrap_or(0).saturating_add(1);
        let name = name.and_then(clean_name).unwrap_or_else(|| format!("Cluster{id}"));
        if let Some(p) = parent {
            self.cluster_mut(p)?.devices.retain(|d| !devices.contains(d));
        }
        for c in &mut self.clusters {
            if clusters.contains(&c.id) {
                c.parent = Some(id);
            }
        }
        self.clusters.push(Cluster { id, name, x: finite(sum.0 / n), y: finite(sum.1 / n), devices, parent });
        Ok(id)
    }

    pub fn rename_cluster(&mut self, id: u32, name: &str) -> Result<()> {
        let name = clean_name(name).ok_or_else(|| ModelError::Invalid("a cluster needs a name".into()))?;
        self.cluster_mut(id)?.name = name;
        Ok(())
    }

    /// Removes a cluster; what it held goes back to the view it sat in.
    pub fn dissolve_cluster(&mut self, id: u32) -> Result<Cluster> {
        let i = self.clusters.iter().position(|c| c.id == id).ok_or_else(|| ModelError::Invalid(format!("no cluster {id}")))?;
        let c = self.clusters.remove(i);
        if let Some(p) = c.parent
            && let Ok(parent) = self.cluster_mut(p)
        {
            parent.devices.extend(c.devices.iter().copied());
        }
        for x in &mut self.clusters {
            if x.parent == Some(id) {
                x.parent = c.parent;
            }
        }
        for a in &mut self.annotations {
            if a.cluster == Some(id) {
                a.cluster = c.parent;
            }
        }
        Ok(c)
    }

    /// Repairs clusters and drawing points from a hostile file: unknown or repeated members,
    /// unknown parents and parent loops, oversized lists.
    pub(crate) fn sanitize_workspace(&mut self) {
        for a in &mut self.annotations {
            let max = if a.shape == Shape::Polygon { MAX_POLYGON_POINTS } else { MAX_FREEHAND_POINTS };
            a.points.truncate(max);
            for p in &mut a.points {
                *p = finite_point(*p);
            }
        }
        self.clusters.truncate(MAX_CLUSTERS);
        let mut ids = BTreeSet::new();
        self.clusters.retain(|c| ids.insert(c.id));
        let devices: BTreeSet<DeviceId> = self.devices.iter().map(|d| d.id).collect();
        let mut claimed = BTreeSet::new();
        for c in &mut self.clusters {
            c.name = clean_name(&c.name).unwrap_or_else(|| format!("Cluster{}", c.id));
            c.x = finite(c.x);
            c.y = finite(c.y);
            c.devices.retain(|d| devices.contains(d) && claimed.insert(*d));
            if c.parent.is_some_and(|p| p == c.id || !ids.contains(&p)) {
                c.parent = None;
            }
        }
        // Break parent loops: a cluster that leads back to itself goes to the top level.
        for i in 0..self.clusters.len() {
            let Some(start) = self.clusters.get(i).map(|c| c.id) else { continue };
            let mut at = self.clusters.get(i).and_then(|c| c.parent);
            let mut steps = 0;
            let mut looped = false;
            while let Some(p) = at {
                steps += 1;
                if p == start || steps > self.clusters.len() {
                    looped = true;
                    break;
                }
                at = self.cluster(p).and_then(|c| c.parent);
            }
            if looped && let Some(c) = self.clusters.get_mut(i) {
                c.parent = None;
            }
        }
        for a in &mut self.annotations {
            if a.cluster.is_some_and(|c| !ids.contains(&c)) {
                a.cluster = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceKind, End, file};

    #[test]
    fn simplify_keeps_corners_and_drops_straight_runs() {
        let line: Vec<[f32; 2]> = (0..100).map(|i| [i as f32, 0.0]).chain((1..100).map(|i| [99.0, i as f32])).collect();
        let s = simplify(&line, 0.5);
        assert_eq!(s, vec![[0.0, 0.0], [99.0, 0.0], [99.0, 99.0]]);
        assert!(simplify(&[], 1.0).is_empty());
        assert_eq!(thin((0..1000).map(|i| [i as f32, (i % 7) as f32]).collect(), 10).len(), 10);
    }

    #[test]
    fn shape_points_are_relative_capped_and_checked() {
        let (x, y, p) = shape_points(Shape::Polygon, &[[10.0, 20.0], [30.0, 20.0], [20.0, 40.0], [10.0, 20.0]]).unwrap();
        assert_eq!((x, y), (10.0, 20.0));
        assert_eq!(p, vec![[0.0, 0.0], [20.0, 0.0], [10.0, 20.0]]);
        assert!(shape_points(Shape::Polygon, &[[0.0, 0.0], [1.0, 1.0]]).is_err());
        assert!(shape_points(Shape::Polygon, &(0..100).map(|i| [i as f32, (i * i) as f32]).collect::<Vec<_>>()).is_err());
        assert!(shape_points(Shape::Freehand, &[[f32::NAN, 0.0], [1.0, 1.0]]).is_err());
        let zigzag: Vec<[f32; 2]> = (0..50_000).map(|i| [i as f32, if i % 2 == 0 { 0.0 } else { 50.0 }]).collect();
        let (_, _, z) = shape_points(Shape::Freehand, &zigzag).unwrap();
        assert!(z.len() <= MAX_FREEHAND_POINTS);
        assert!(shape_points(Shape::Rectangle, &zigzag).is_err());
    }

    fn lab() -> (Topology, Vec<DeviceId>) {
        let mut t = Topology::new();
        let ids: Vec<DeviceId> = (0..4).map(|i| t.add_device(DeviceKind::Pc, None, i as f32 * 100.0, 0.0).unwrap()).collect();
        let sw = t.add_device(DeviceKind::Switch, None, 0.0, 200.0).unwrap();
        for (i, d) in ids.iter().enumerate() {
            t.connect(End { device: *d, port: 0 }, End { device: sw, port: i }).unwrap();
        }
        (t, ids)
    }

    #[test]
    fn clusters_nest_show_and_dissolve() {
        let (mut t, ids) = lab();
        let a = t.create_cluster(&ids[..2], &[], Some("  Office  ")).unwrap();
        assert_eq!(t.cluster(a).unwrap().name, "Office");
        assert_eq!((t.cluster(a).unwrap().x, t.cluster(a).unwrap().y), (50.0, 0.0));
        assert_eq!(t.shown_as(ids[0], None), Some(Shown::Cluster(a)));
        assert_eq!(t.shown_as(ids[0], Some(a)), Some(Shown::Device));
        assert_eq!(t.shown_as(ids[2], Some(a)), None);
        // A cluster inside it, and one holding both a device and a cluster.
        let b = t.create_cluster(&ids[1..2], &[], None).unwrap();
        assert_eq!(t.cluster(b).unwrap().parent, Some(a));
        assert_eq!(t.cluster_path(Some(b)), vec![a, b]);
        assert_eq!(t.shown_as(ids[1], None), Some(Shown::Cluster(a)));
        assert_eq!(t.shown_as(ids[1], Some(a)), Some(Shown::Cluster(b)));
        assert_eq!(t.shown_map(Some(a)).len(), 2);
        // Mixed levels are refused.
        assert!(t.create_cluster(&[ids[0], ids[2]], &[], None).is_err());
        let c = t.create_cluster(&[ids[2]], &[a], Some("Floor")).unwrap();
        assert_eq!(t.cluster(a).unwrap().parent, Some(c));
        assert_eq!(t.shown_as(ids[1], None), Some(Shown::Cluster(c)));
        t.dissolve_cluster(c).unwrap();
        assert_eq!(t.cluster(a).unwrap().parent, None);
        assert_eq!(t.cluster_of(ids[2]), None);
        t.dissolve_cluster(a).unwrap();
        assert_eq!(t.cluster(b).unwrap().parent, None);
        assert_eq!(t.cluster_of(ids[0]), None);
        assert!(t.create_cluster(&[], &[], None).is_err());
        assert!(t.create_cluster(&[DeviceId(999)], &[], None).is_err());
        assert!(t.rename_cluster(b, " \n ").is_err());
        // Removing a device takes it out of its cluster.
        t.remove_device(ids[1]).unwrap();
        assert!(t.cluster(b).unwrap().devices.is_empty());
    }

    #[test]
    fn clusters_and_paths_round_trip_and_hostile_files_are_repaired() {
        let (mut t, ids) = lab();
        let a = t.create_cluster(&ids[..2], &[], Some("Lab A")).unwrap();
        t.add_path(Shape::Polygon, &[[0.0, 0.0], [40.0, 0.0], [0.0, 30.0]], Some("#ff0000"), Some(a)).unwrap();
        t.add_path(Shape::Freehand, &[[0.0, 0.0], [5.0, 9.0], [12.0, 3.0]], None, None).unwrap();
        let s = file::to_json(&t).unwrap();
        assert_eq!(file::from_json(&s).unwrap(), t);
        // Old files without clusters or points still open.
        let mut v: serde_json::Value = serde_json::from_str(&s).unwrap();
        v["topology"].as_object_mut().unwrap().remove("clusters");
        assert!(file::from_json(&v.to_string()).unwrap().clusters.is_empty());
        // Loops, unknown members and parents, repeated ids.
        v["topology"]["clusters"] = serde_json::json!([
            {"id": 1, "name": "", "x": 1e30, "y": 0, "devices": [ids[0], 9999, ids[0]], "parent": 2},
            {"id": 2, "name": "B", "x": 0, "y": 0, "devices": [ids[0], ids[1]], "parent": 1},
            {"id": 2, "name": "dup", "x": 0, "y": 0},
            {"id": 3, "name": "C", "x": 0, "y": 0, "parent": 3},
            {"id": 4, "name": "D", "x": 0, "y": 0, "parent": 77}
        ]);
        let r = file::from_json(&v.to_string()).unwrap();
        assert_eq!(r.clusters.len(), 4);
        assert_eq!(r.clusters[0].devices, vec![ids[0]]);
        assert_eq!(r.clusters[1].devices, vec![ids[1]]);
        assert!(r.clusters.iter().all(|c| c.parent.is_none_or(|p| r.cluster(p).is_some_and(|x| x.parent != Some(c.id)))));
        assert!(r.clusters.iter().all(|c| !c.name.is_empty() && c.x.abs() <= 1e6));
        for c in &r.clusters {
            for d in &ids {
                let _ = r.shown_as(*d, Some(c.id));
            }
        }
    }
}
