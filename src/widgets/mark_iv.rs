//! Ultracortex Mark IV medium frame (official STL, decimated).
//! Mesh: `resources/ultracortex_mark_iv/frame.bin` (M4_Medium_Front + Back).
//! 35 named holes are the circular INSERT sockets (electrode nodes), not
//! decorative lattice openings. Default 3/4 camera, slightly above; Head Plot drag orbits.

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Vec2};
use std::collections::{HashSet, VecDeque};
use std::sync::OnceLock;

pub const HEADSET_MARK_IV: &str = "Ultracortex Mark IV";
pub const HEADSET_NAME: &str = HEADSET_MARK_IV;
pub const DEFAULT_SITES: [&str; 8] = ["Fp1", "Fp2", "C3", "C4", "P7", "P8", "O1", "O2"];

/// 3/4 view, slightly above: headset lattice and the 8 wired holes both read.
pub const VIEW_YAW: f32 = 0.58;
pub const VIEW_PITCH: f32 = -0.48;

const BIN: &[u8] = include_bytes!("../../resources/ultracortex_mark_iv/frame.bin");

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
}

pub type Orbit = Camera;

impl Default for Camera {
    fn default() -> Self {
        Self::THREE_QUARTER
    }
}

impl Camera {
    pub const THREE_QUARTER: Self = Self {
        yaw: VIEW_YAW,
        pitch: VIEW_PITCH,
    };

    pub fn drag(&mut self, delta: Vec2) {
        self.yaw += delta.x * 0.012;
        self.pitch = (self.pitch + delta.y * 0.012).clamp(-1.15, 0.35);
    }

    pub fn rotate(self, p: [f32; 3]) -> [f32; 3] {
        let (cy, sy) = (self.yaw.cos(), self.yaw.sin());
        let x1 = p[0] * cy - p[1] * sy;
        let y1 = p[0] * sy + p[1] * cy;
        let z1 = p[2];
        let (cp, sp) = (self.pitch.cos(), self.pitch.sin());
        let y2 = y1 * cp - z1 * sp;
        let z2 = y1 * sp + z1 * cp;
        [x1, y2, z2]
    }
}

#[derive(Clone, Debug)]
pub struct Hole {
    pub name: String,
    pub p: [f32; 3],
}

pub struct FrameMesh {
    pub verts: Vec<[f32; 3]>,
    pub fnorms: Vec<[f32; 3]>,
    pub faces: Vec<[u32; 3]>,
    pub holes: Vec<Hole>,
}

fn u32le(buf: &[u8], off: &mut usize) -> Option<u32> {
    let s = *off;
    if s + 4 > buf.len() {
        return None;
    }
    *off += 4;
    Some(u32::from_le_bytes(buf[s..s + 4].try_into().ok()?))
}

fn f32le(buf: &[u8], off: &mut usize) -> Option<f32> {
    let s = *off;
    if s + 4 > buf.len() {
        return None;
    }
    *off += 4;
    Some(f32::from_le_bytes(buf[s..s + 4].try_into().ok()?))
}

fn vec3(buf: &[u8], off: &mut usize) -> Option<[f32; 3]> {
    Some([f32le(buf, off)?, f32le(buf, off)?, f32le(buf, off)?])
}

fn load_bin(buf: &[u8]) -> Option<FrameMesh> {
    if buf.len() < 16 || &buf[0..4] != b"M4FR" {
        return None;
    }
    let mut off = 4;
    let ver = u32le(buf, &mut off)?;
    if ver != 1 {
        return None;
    }
    let nverts = u32le(buf, &mut off)? as usize;
    let nfaces = u32le(buf, &mut off)? as usize;
    let mut verts = Vec::with_capacity(nverts);
    for _ in 0..nverts {
        verts.push(vec3(buf, &mut off)?);
    }
    let mut fnorms = Vec::with_capacity(nfaces);
    for _ in 0..nfaces {
        fnorms.push(vec3(buf, &mut off)?);
    }
    let mut faces = Vec::with_capacity(nfaces);
    for _ in 0..nfaces {
        faces.push([
            u32le(buf, &mut off)?,
            u32le(buf, &mut off)?,
            u32le(buf, &mut off)?,
        ]);
    }
    let _ = off;
    // File hole table is ideal 10-20 on r≈0.96. Names sit on INSERT sockets.
    let holes = insert_sockets();
    Some(FrameMesh {
        verts,
        fnorms,
        faces,
        holes,
    })
}

/// 35 circular INSERT socket centers in frame.bin space
/// (+X right, −Y anterior, +Z up).
///
/// Derived from official OpenBCI `M4H6_Medium Node Array.stl` (35 disconnected
/// node solids at the actual Mark IV insert locations), transformed with the
/// same bbox-center / X-flip / max-radius=1 as M4_Medium_Front+Back → frame.bin.
/// Each center sits in an empty circular rim (r≈0.12) of the printed node —
/// not a decorative lattice opening, not a ray-snap onto nearby verts, and not
/// the ideal r=0.96 10-20 sphere. Names are 10-20/10-10 by anatomy
/// (Fp1 left-front forehead, O posterior, Cz up) — not a greedy 10-20 snap.
const INSERT_HOLES: [(&str, [f32; 3]); 35] = [
    ("Fp1", [-0.407180, -0.509746, 0.037802]),
    ("Fp2", [0.414591, -0.517388, 0.040614]),
    ("F7", [-0.616524, -0.245182, -0.087566]),
    ("F3", [-0.267197, -0.283586, 0.299279]),
    ("Fz", [-0.002140, -0.564037, 0.219643]),
    ("F4", [0.269280, -0.289258, 0.302449]),
    ("F8", [0.625861, -0.247543, -0.086253]),
    ("FT7", [-0.585197, -0.443445, -0.340458]),
    ("FC5", [-0.330066, -0.703388, -0.120800]),
    ("Fpz", [-0.001630, -0.768451, -0.022334]),
    ("FC6", [0.334095, -0.709880, -0.122557]),
    ("FT8", [0.591714, -0.447986, -0.339344]),
    ("T7", [-0.700152, 0.002600, -0.347569]),
    ("C3", [-0.520944, 0.002466, 0.187158]),
    ("Cz", [-0.001196, 0.002249, 0.441163]),
    ("C4", [0.526139, 0.002845, 0.185804]),
    ("T8", [0.702304, 0.002579, -0.350121]),
    ("TP7", [-0.601896, 0.458905, -0.350679]),
    ("P7", [-0.649356, 0.258750, -0.080743]),
    ("Pz", [0.001095, 0.757248, -0.029398]),
    ("P8", [0.644480, 0.260224, -0.081404]),
    ("TP8", [0.593524, 0.457163, -0.350079]),
    ("P3", [-0.282980, 0.304257, 0.346607]),
    ("P4", [0.284116, 0.305136, 0.342809]),
    ("PO3", [-0.336383, 0.714337, -0.124809]),
    ("PO4", [0.331977, 0.708350, -0.129170]),
    ("O1", [-0.441245, 0.540079, 0.060832]),
    ("Oz", [0.001170, 0.586293, 0.234527]),
    ("O2", [0.436298, 0.536364, 0.057162]),
    ("AF7", [-0.256955, -0.799661, -0.340073]),
    ("AF8", [0.257060, -0.801834, -0.341984]),
    ("AFz", [-0.001046, -0.848105, -0.334220]),
    ("PO7", [-0.251448, 0.797272, -0.339165]),
    ("Iz", [0.002242, 0.842137, -0.330715]),
    ("PO8", [0.253805, 0.791455, -0.340214]),
];

fn insert_sockets() -> Vec<Hole> {
    INSERT_HOLES
        .iter()
        .map(|(name, p)| Hole {
            name: (*name).to_string(),
            p: *p,
        })
        .collect()
}

fn len3(p: [f32; 3]) -> f32 {
    (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    len3(sub3(a, b))
}

fn dist_point_seg(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
    let ab = sub3(b, a);
    let l2 = dot3(ab, ab).max(1e-12);
    let t = (dot3(sub3(p, a), ab) / l2).clamp(0.0, 1.0);
    dist3(p, add3(a, mul3(ab, t)))
}

fn dist_point_seg2(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq().max(1e-12);
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn orient2(a: Pos2, b: Pos2, c: Pos2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn dist_seg_seg2(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> f32 {
    let o1 = orient2(a, b, c);
    let o2 = orient2(a, b, d);
    let o3 = orient2(c, d, a);
    let o4 = orient2(c, d, b);
    if o1 * o2 <= 0.0 && o3 * o4 <= 0.0 {
        return 0.0;
    }
    dist_point_seg2(a, c, d)
        .min(dist_point_seg2(b, c, d))
        .min(dist_point_seg2(c, a, b))
        .min(dist_point_seg2(d, a, b))
}

fn dist_tri_seg2(ca: Pos2, cb: Pos2, cc: Pos2, a: Pos2, b: Pos2) -> f32 {
    let c = Pos2::new(
        (ca.x + cb.x + cc.x) * (1.0 / 3.0),
        (ca.y + cb.y + cc.y) * (1.0 / 3.0),
    );
    dist_point_seg2(c, a, b)
        .min(dist_point_seg2(ca, a, b))
        .min(dist_point_seg2(cb, a, b))
        .min(dist_point_seg2(cc, a, b))
        .min(dist_seg_seg2(ca, cb, a, b))
        .min(dist_seg_seg2(cb, cc, a, b))
        .min(dist_seg_seg2(cc, ca, a, b))
}

/// Empty interior + circular ring of mesh verts in the local tangent plane.
/// True for INSERT sockets; false for a lattice rib or the ideal r=0.96 sphere.
fn sits_on_circular_rim(verts: &[[f32; 3]], center: [f32; 3]) -> bool {
    let nlen = len3(center);
    if nlen < 1e-6 {
        return false;
    }
    let n = [center[0] / nlen, center[1] / nlen, center[2] / nlen];
    let mut near = 0u32;
    let mut ring: Vec<f32> = Vec::new();
    for v in verts {
        let d = sub3(*v, center);
        let axial = dot3(d, n).abs();
        if axial > 0.05 {
            continue;
        }
        let rad = (len3(d) * len3(d) - axial * axial).max(0.0).sqrt();
        if rad < 0.025 {
            near += 1;
        }
        if (0.08..=0.16).contains(&rad) {
            ring.push(rad);
        }
    }
    if near > 2 || ring.len() < 20 {
        return false;
    }
    let mean = ring.iter().sum::<f32>() / ring.len() as f32;
    let var = ring.iter().map(|r| (r - mean) * (r - mean)).sum::<f32>() / ring.len() as f32;
    let std = var.sqrt();
    std < 0.035 && (0.09..=0.15).contains(&mean)
}

static MESH: OnceLock<FrameMesh> = OnceLock::new();

pub fn mesh() -> &'static FrameMesh {
    MESH.get_or_init(|| {
        load_bin(BIN).unwrap_or_else(|| FrameMesh {
            verts: Vec::new(),
            fnorms: Vec::new(),
            faces: Vec::new(),
            holes: Vec::new(),
        })
    })
}

pub fn default_map() -> [String; 8] {
    DEFAULT_SITES.map(|s| s.to_string())
}

pub fn is_hole(name: &str) -> bool {
    mesh()
        .holes
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case(name))
}

pub fn hole_index(name: &str) -> Option<usize> {
    mesh()
        .holes
        .iter()
        .position(|h| h.name.eq_ignore_ascii_case(name))
}

pub fn hole_name(index: usize) -> Option<&'static str> {
    mesh().holes.get(index).map(|h| h.name.as_str())
}

pub fn channel_at(map: &[String], name: &str) -> Option<usize> {
    map.iter().position(|h| h.eq_ignore_ascii_case(name))
}

pub fn canvas_scale(rect: Rect) -> f32 {
    (rect.width().min(rect.height()) * 0.46).max(36.0)
}

#[derive(Clone, Copy, Debug)]
pub struct Projected {
    pub pos: Pos2,
    pub depth: f32,
}

pub fn project(p: [f32; 3], cam: Camera, center: Pos2, scale: f32) -> Projected {
    let r = cam.rotate(p);
    let dist = 3.4;
    let w = dist / (dist - r[2]).max(0.35);
    Projected {
        pos: Pos2::new(center.x + r[0] * scale * w, center.y - r[1] * scale * w),
        depth: r[2],
    }
}

fn shade(n_cam: [f32; 3]) -> Color32 {
    let light = [0.38, -0.42, 0.82];
    let nd = (n_cam[0] * light[0] + n_cam[1] * light[1] + n_cam[2] * light[2])
        .abs()
        .clamp(0.0, 1.0);
    let k = 0.16 + 0.84 * nd;
    let lo = 0x2a as f32;
    let hi = 0x78 as f32;
    let v = (lo + (hi - lo) * k) as u8;
    Color32::from_rgb(v, v.saturating_add(1), v)
}

fn project_cam(r: [f32; 3], center: Pos2, scale: f32) -> Pos2 {
    let dist = 3.4;
    let w = dist / (dist - r[2]).max(0.35);
    Pos2::new(center.x + r[0] * scale * w, center.y - r[1] * scale * w)
}

/// Quiet anatomical scalp in the same frame as the Mark IV
/// (+X right, −Y anterior, +Z up, radius 1 is the headset). Head sits *inside*
/// the lattice so the frame reads as worn, not a floating cage.
struct SolidMesh {
    verts: Vec<[f32; 3]>,
    fnorms: Vec<[f32; 3]>,
    faces: Vec<[u32; 3]>,
}

fn lat_long_ellipsoid(rx: f32, ry: f32, rz: f32, nu: usize, nv: usize) -> SolidMesh {
    let mut verts = Vec::with_capacity((nu + 1) * (nv + 1));
    for i in 0..=nu {
        let theta = std::f32::consts::PI * i as f32 / nu as f32;
        let st = theta.sin();
        let ct = theta.cos();
        for j in 0..=nv {
            let phi = 2.0 * std::f32::consts::PI * j as f32 / nv as f32;
            // phi=0 → −Y (nose / anterior)
            let x = rx * st * phi.sin();
            let y = -ry * st * phi.cos();
            let z = rz * ct;
            verts.push([x, y, z]);
        }
    }
    // Nose: push the anterior pole slightly forward and down.
    for v in verts.iter_mut() {
        let anterior = (-v[1] / ry).clamp(0.0, 1.0);
        let mid = (1.0 - (v[2] / rz).abs()).clamp(0.0, 1.0);
        if anterior > 0.72 && v[2] < 0.12 && v[2] > -0.28 {
            let k = ((anterior - 0.72) / 0.28) * mid;
            v[1] -= 0.11 * k;
            v[2] -= 0.03 * k;
        }
        // Ear notches near F7/F8: small C-notch in X.
        let lat = v[0].abs() / rx;
        if lat > 0.82 && v[1].abs() < 0.22 * ry && v[2].abs() < 0.18 * rz {
            let k = ((lat - 0.82) / 0.18).clamp(0.0, 1.0);
            v[0] *= 1.0 - 0.08 * k;
        }
        // Neck: flatten the bottom.
        if v[2] < -0.55 * rz {
            let k = ((-0.55 * rz - v[2]) / (0.45 * rz)).clamp(0.0, 1.0);
            v[0] *= 1.0 - 0.35 * k;
            v[1] *= 1.0 - 0.20 * k;
            v[2] = -0.55 * rz - 0.18 * k * rz;
        }
    }
    let cols = nv + 1;
    let mut faces = Vec::new();
    for i in 0..nu {
        for j in 0..nv {
            let a = (i * cols + j) as u32;
            let b = a + 1;
            let c = a + cols as u32;
            let d = c + 1;
            faces.push([a, c, b]);
            faces.push([b, c, d]);
        }
    }
    let mut fnorms = Vec::with_capacity(faces.len());
    for f in &faces {
        let a = verts[f[0] as usize];
        let b = verts[f[1] as usize];
        let c = verts[f[2] as usize];
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-8);
        fnorms.push([n[0] / len, n[1] / len, n[2] / len]);
    }
    SolidMesh {
        verts,
        fnorms,
        faces,
    }
}

fn head_mesh() -> &'static SolidMesh {
    static M: OnceLock<SolidMesh> = OnceLock::new();
    M.get_or_init(|| {
        // Smaller than the radius-1 lattice so the scalp fills the helmet cavity
        // with the cage outside. Drop in Z so the neck hangs below the rim and
        // the crown sits under the vault (headset ON the head, not through it).
        let mut m = lat_long_ellipsoid(0.58, 0.68, 0.60, 18, 28);
        for v in m.verts.iter_mut() {
            v[2] -= 0.10;
        }
        m
    })
}

fn shade_solid(n_cam: [f32; 3], lo: [u8; 3], hi: [u8; 3], alpha: u8) -> Color32 {
    let light = [0.38, -0.42, 0.82];
    let nd = (n_cam[0] * light[0] + n_cam[1] * light[1] + n_cam[2] * light[2])
        .abs()
        .clamp(0.0, 1.0);
    let k = 0.18 + 0.82 * nd;
    let r = (lo[0] as f32 + (hi[0] as f32 - lo[0] as f32) * k) as u8;
    let g = (lo[1] as f32 + (hi[1] as f32 - lo[1] as f32) * k) as u8;
    let b = (lo[2] as f32 + (hi[2] as f32 - lo[2] as f32) * k) as u8;
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

fn paint_solid(
    painter: &egui::Painter,
    rect: Rect,
    cam: Camera,
    mesh: &SolidMesh,
    lo: [u8; 3],
    hi: [u8; 3],
    alpha: u8,
) {
    if mesh.faces.is_empty() {
        return;
    }
    let center = rect.center();
    let scale = canvas_scale(rect);
    let mut cam_v: Vec<[f32; 3]> = vec![[0.0; 3]; mesh.verts.len()];
    for (i, v) in mesh.verts.iter().enumerate() {
        cam_v[i] = cam.rotate(*v);
    }
    let mut order: Vec<(f32, usize)> = Vec::with_capacity(mesh.faces.len());
    for (i, face) in mesh.faces.iter().enumerate() {
        let a = cam_v[face[0] as usize];
        let b = cam_v[face[1] as usize];
        let c = cam_v[face[2] as usize];
        let z = (a[2] + b[2] + c[2]) * (1.0 / 3.0);
        order.push((z, i));
    }
    order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut gpu = Mesh::default();
    gpu.vertices.reserve(mesh.faces.len() * 3);
    gpu.indices.reserve(mesh.faces.len() * 3);
    for &(_, i) in &order {
        let face = mesh.faces[i];
        let n = cam.rotate(mesh.fnorms[i]);
        if n[2] < -0.12 {
            continue;
        }
        let col = shade_solid(n, lo, hi, alpha);
        let base = gpu.vertices.len() as u32;
        for k in 0..3 {
            let r = cam_v[face[k] as usize];
            let pr = project_cam(r, center, scale);
            gpu.vertices.push(egui::epaint::Vertex {
                pos: pr,
                uv: Pos2::ZERO,
                color: col,
            });
        }
        gpu.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    painter.add(egui::Shape::mesh(gpu));
}

/// Opaque anatomical scalp; the caller paints the Mark IV lattice on top.
pub fn paint_head(painter: &egui::Painter, rect: Rect, cam: Camera) {
    paint_solid(
        painter,
        rect,
        cam,
        head_mesh(),
        [0x2a, 0x28, 0x26],
        [0x6a, 0x5e, 0x56],
        255,
    );
}

/// Tube around the mesh geodesic between two labeled inserts. Wider than a
/// face (edge ≈ 0.037) so the bar drops; narrower than a node so rims stay.
const PAIR_STRUT_TUBE: f32 = 0.034;
/// Keep the circular INSERT rim; only the bar between holes is skipped.
const PAIR_STRUT_RIM: f32 = 0.12;
/// Endpoint neighborhoods for the geodesic search (both ends near those holes).
const PAIR_STRUT_END: f32 = 0.14;
/// Pair views only: also drop faces whose screen centroid sits on the 2D
/// segment between the two labeled projected holes (remaining lattice ridge).
const PAIR_SCREEN_PX: f32 = 4.0;

pub fn hole_pos(name: &str) -> Option<[f32; 3]> {
    mesh()
        .holes
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.p)
}

fn mesh_adj() -> &'static [Vec<u32>] {
    static ADJ: OnceLock<Vec<Vec<u32>>> = OnceLock::new();
    ADJ.get_or_init(|| {
        let m = mesh();
        let mut adj = vec![Vec::new(); m.verts.len()];
        for f in &m.faces {
            adj[f[0] as usize].push(f[1]);
            adj[f[0] as usize].push(f[2]);
            adj[f[1] as usize].push(f[0]);
            adj[f[1] as usize].push(f[2]);
            adj[f[2] as usize].push(f[0]);
            adj[f[2] as usize].push(f[1]);
        }
        adj
    })
}

fn verts_near(p: [f32; 3], r: f32) -> Vec<usize> {
    mesh()
        .verts
        .iter()
        .enumerate()
        .filter_map(|(i, v)| (dist3(*v, p) < r).then_some(i))
        .collect()
}

/// Short mesh geodesic between the two insert neighborhoods.
fn pair_geodesic_polyline(a: [f32; 3], b: [f32; 3]) -> Vec<[f32; 3]> {
    let src = verts_near(a, PAIR_STRUT_END);
    let dst = verts_near(b, PAIR_STRUT_END);
    if src.is_empty() || dst.is_empty() {
        return Vec::new();
    }
    let dst_set: HashSet<usize> = dst.iter().copied().collect();
    let src_set: HashSet<usize> = src.iter().copied().collect();
    let adj = mesh_adj();
    let mut prev: Vec<Option<usize>> = vec![None; adj.len()];
    let mut seen = vec![false; adj.len()];
    let mut q = VecDeque::new();
    for &s in &src {
        seen[s] = true;
        q.push_back(s);
    }
    let mut found = None;
    while let Some(u) = q.pop_front() {
        if dst_set.contains(&u) && !src_set.contains(&u) {
            found = Some(u);
            break;
        }
        for &v in &adj[u] {
            let vi = v as usize;
            if seen[vi] {
                continue;
            }
            seen[vi] = true;
            prev[vi] = Some(u);
            q.push_back(vi);
        }
    }
    let Some(end) = found else {
        return Vec::new();
    };
    let mut idx = Vec::new();
    let mut cur = Some(end);
    while let Some(u) = cur {
        idx.push(u);
        cur = prev[u];
    }
    idx.reverse();
    idx.into_iter().map(|i| mesh().verts[i]).collect()
}

fn face_centroid(mesh: &FrameMesh, face: [u32; 3]) -> [f32; 3] {
    let a = mesh.verts[face[0] as usize];
    let b = mesh.verts[face[1] as usize];
    let c = mesh.verts[face[2] as usize];
    [
        (a[0] + b[0] + c[0]) * (1.0 / 3.0),
        (a[1] + b[1] + c[1]) * (1.0 / 3.0),
        (a[2] + b[2] + c[2]) * (1.0 / 3.0),
    ]
}

/// Lattice faces whose centroid sits on the short geodesic between `hole_a`
/// and `hole_b`. Insert rims stay so the two discs still sit in holes.
pub fn pair_strut_skip_indices(hole_a: &str, hole_b: &str) -> HashSet<usize> {
    let Some(pa) = hole_pos(hole_a) else {
        return HashSet::new();
    };
    let Some(pb) = hole_pos(hole_b) else {
        return HashSet::new();
    };
    let path = pair_geodesic_polyline(pa, pb);
    if path.len() < 2 {
        return HashSet::new();
    }
    let mesh = mesh();
    let mut skip = HashSet::new();
    for (i, face) in mesh.faces.iter().enumerate() {
        let c = face_centroid(mesh, *face);
        if dist3(c, pa) < PAIR_STRUT_RIM || dist3(c, pb) < PAIR_STRUT_RIM {
            continue;
        }
        let mut dmin = f32::MAX;
        for w in path.windows(2) {
            dmin = dmin.min(dist_point_seg(c, w[0], w[1]));
        }
        if dmin < PAIR_STRUT_TUBE {
            skip.insert(i);
        }
    }
    skip
}

/// Faces whose projected centroid is within PAIR_SCREEN_PX of the 2D segment
/// between the two labeled holes. Pair views only; Head Plot does not call this.
pub fn pair_screen_skip_indices(
    hole_a: &str,
    hole_b: &str,
    rect: Rect,
    cam: Camera,
) -> HashSet<usize> {
    let Some(pa) = hole_pos(hole_a) else {
        return HashSet::new();
    };
    let Some(pb) = hole_pos(hole_b) else {
        return HashSet::new();
    };
    let center = rect.center();
    let scale = canvas_scale(rect);
    let a2 = project(pa, cam, center, scale).pos;
    let b2 = project(pb, cam, center, scale).pos;
    let mesh = mesh();
    let mut skip = HashSet::new();
    for (i, face) in mesh.faces.iter().enumerate() {
        let c3 = face_centroid(mesh, *face);
        if dist3(c3, pa) < PAIR_STRUT_RIM || dist3(c3, pb) < PAIR_STRUT_RIM {
            continue;
        }
        let ca = project(mesh.verts[face[0] as usize], cam, center, scale).pos;
        let cb = project(mesh.verts[face[1] as usize], cam, center, scale).pos;
        let cc = project(mesh.verts[face[2] as usize], cam, center, scale).pos;
        if dist_tri_seg2(ca, cb, cc, a2, b2) <= PAIR_SCREEN_PX {
            skip.insert(i);
        }
    }
    skip
}

fn paint_frame_ex(
    painter: &egui::Painter,
    rect: Rect,
    cam: Camera,
    hide_pair: Option<(&str, &str)>,
) {
    let mesh = mesh();
    if mesh.faces.is_empty() {
        return;
    }
    let skip = match hide_pair {
        Some((a, b)) => {
            let mut skip = pair_strut_skip_indices(a, b);
            skip.extend(pair_screen_skip_indices(a, b, rect, cam));
            skip
        }
        None => HashSet::new(),
    };
    let center = rect.center();
    let scale = canvas_scale(rect);
    let mut cam_v: Vec<[f32; 3]> = vec![[0.0; 3]; mesh.verts.len()];
    for (i, v) in mesh.verts.iter().enumerate() {
        cam_v[i] = cam.rotate(*v);
    }
    let mut order: Vec<(f32, usize)> = Vec::with_capacity(mesh.faces.len());
    for (i, face) in mesh.faces.iter().enumerate() {
        let a = cam_v[face[0] as usize];
        let b = cam_v[face[1] as usize];
        let c = cam_v[face[2] as usize];
        let z = (a[2] + b[2] + c[2]) * (1.0 / 3.0);
        order.push((z, i));
    }
    order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut gpu = Mesh::default();
    gpu.vertices.reserve(mesh.faces.len() * 3);
    gpu.indices.reserve(mesh.faces.len() * 3);
    for &(_, i) in &order {
        if skip.contains(&i) {
            continue;
        }
        let face = mesh.faces[i];
        let n = cam.rotate(mesh.fnorms[i]);
        if n[2] < -0.08 {
            continue;
        }
        let col = shade(n);
        let base = gpu.vertices.len() as u32;
        for k in 0..3 {
            let r = cam_v[face[k] as usize];
            let pr = project_cam(r, center, scale);
            gpu.vertices.push(egui::epaint::Vertex {
                pos: pr,
                uv: Pos2::ZERO,
                color: col,
            });
        }
        gpu.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    painter.add(egui::Shape::mesh(gpu));
}

/// Draw the lit Mark IV lattice (painter's algorithm → egui/wgpu mesh).
pub fn paint_frame(painter: &egui::Painter, rect: Rect, cam: Camera) {
    paint_frame_ex(painter, rect, cam, None);
}

/// Pair views (Left / right, Which first): drop the lattice strut that runs
/// between the two labeled inserts so it cannot read as a path. Head Plot
/// keeps the full frame via `paint_frame`.
pub fn paint_frame_hiding_pair(
    painter: &egui::Painter,
    rect: Rect,
    cam: Camera,
    hole_a: &str,
    hole_b: &str,
) {
    paint_frame_ex(painter, rect, cam, Some((hole_a, hole_b)));
}

#[derive(Clone, Copy, Debug)]
pub struct ProjectedHole {
    pub index: usize,
    pub pos: Pos2,
    pub depth: f32,
}

pub fn project_holes(rect: Rect, cam: Camera) -> Vec<ProjectedHole> {
    let center = rect.center();
    let scale = canvas_scale(rect);
    mesh()
        .holes
        .iter()
        .enumerate()
        .map(|(index, h)| {
            let pr = project(h.p, cam, center, scale);
            ProjectedHole {
                index,
                pos: pr.pos,
                depth: pr.depth,
            }
        })
        .collect()
}

pub fn hit_hole(pointer: Pos2, projected: &[ProjectedHole], slop: f32) -> Option<usize> {
    let mut best = None;
    let mut best_score = f32::MAX;
    for p in projected {
        let d2 = (p.pos - pointer).length_sq();
        if d2 > slop * slop {
            continue;
        }
        let score = d2 - p.depth * 40.0;
        if score < best_score {
            best_score = score;
            best = Some(p.index);
        }
    }
    best
}

/// One named insert on a pair view. Empty / non-pair holes are omitted.
#[derive(Clone, Copy, Debug)]
pub struct LabeledInsert {
    pub name: &'static str,
    pub fill: f32,
    pub railed: bool,
}

/// Name+fill only `labeled` inserts. Disc size matches Head Plot SITE_R.
/// Other lattice holes stay mesh openings. No click-assign.
pub fn paint_labeled_inserts(
    painter: &egui::Painter,
    rect: Rect,
    projected: &[ProjectedHole],
    labeled: &[LabeledInsert],
) {
    const R: f32 = 6.0;
    let mut draw_order: Vec<usize> = (0..projected.len()).collect();
    draw_order.sort_by(|&a, &b| {
        projected[a]
            .depth
            .partial_cmp(&projected[b].depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for idx in draw_order {
        let pr = projected[idx];
        if !rect.expand(8.0).contains(pr.pos) {
            continue;
        }
        let name = match hole_name(pr.index) {
            Some(n) => n,
            None => continue,
        };
        let Some(lab) = labeled.iter().find(|l| l.name.eq_ignore_ascii_case(name)) else {
            continue;
        };
        if lab.railed {
            painter.circle_stroke(
                pr.pos,
                R,
                egui::Stroke::new(1.25_f32, Color32::from_rgb(0x6b, 0x3d, 0x3d)),
            );
        } else {
            let t = lab.fill.clamp(0.0, 1.0);
            let fill = if t > 0.02 {
                let a = (40.0 + t * 200.0) as u8;
                Color32::from_rgba_unmultiplied(0xb0, 0x8d, 0x57, a)
            } else {
                Color32::from_rgb(0x3d, 0x3d, 0x3d)
            };
            painter.circle_filled(pr.pos, R, fill);
            painter.circle_stroke(
                pr.pos,
                R,
                egui::Stroke::new(1.0_f32, Color32::from_rgb(0x3d, 0x3d, 0x3d)),
            );
        }
        painter.text(
            pr.pos,
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(11.0),
            Color32::from_rgb(0xe6, 0xe6, 0xe6),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_bin_loads_mark_iv_lattice() {
        let m = mesh();
        assert!(m.verts.len() > 500, "verts {}", m.verts.len());
        assert!(m.faces.len() > 500, "faces {}", m.faces.len());
        assert_eq!(m.faces.len(), m.fnorms.len());
        assert_eq!(HEADSET_NAME, "Ultracortex Mark IV");
        assert!(!HEADSET_NAME.contains("version"));
    }

    #[test]
    fn head_sits_inside_the_mark_iv() {
        let h = head_mesh();
        assert!(h.faces.len() > 200, "head faces {}", h.faces.len());
        let rmax = h
            .verts
            .iter()
            .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
            .fold(0.0_f32, f32::max);
        assert!(
            rmax < 0.85 && rmax > 0.5,
            "head must sit clearly inside the radius-1 lattice, r={rmax}"
        );
        let hymin = h.verts.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        let hzmin = h.verts.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
        let hzmax = h.verts.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        assert!(
            hymin < -0.6,
            "nose/anterior must stick forward −Y, ymin={hymin}"
        );
        assert!(
            hzmin < -0.4,
            "neck must sit below the rim −Z, min z={hzmin}"
        );
        assert!(
            hzmax < 0.8 && hzmax > 0.35,
            "crown under Cz (~0.8) still up +Z, max z={hzmax}"
        );
    }

    #[test]
    fn mesh_has_depth_not_a_2d_oval() {
        let m = mesh();
        let mut zmin = f32::MAX;
        let mut zmax = f32::MIN;
        let mut xmin = f32::MAX;
        let mut xmax = f32::MIN;
        let mut ymin = f32::MAX;
        let mut ymax = f32::MIN;
        for p in &m.verts {
            xmin = xmin.min(p[0]);
            xmax = xmax.max(p[0]);
            ymin = ymin.min(p[1]);
            ymax = ymax.max(p[1]);
            zmin = zmin.min(p[2]);
            zmax = zmax.max(p[2]);
        }
        assert!(xmax - xmin > 0.8, "x span {}", xmax - xmin);
        assert!(ymax - ymin > 0.8, "y span {}", ymax - ymin);
        assert!(zmax - zmin > 0.4, "z span {zmax}..{zmin} — not a 2D oval");
    }

    #[test]
    fn thirty_five_holes_include_o1_o2_and_default_8() {
        let names: Vec<&str> = mesh().holes.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names.len(), 35, "{names:?}");
        for need in [
            "Fp1", "Fp2", "F7", "F8", "C3", "C4", "P3", "P4", "O1", "O2", "Cz",
        ] {
            assert!(names.contains(&need), "missing {need} in {names:?}");
        }
    }

    #[test]
    fn fp1_is_left_o_is_posterior() {
        let h = |n: &str| mesh().holes.iter().find(|h| h.name == n).unwrap().p;
        let fp1 = h("Fp1");
        let fp2 = h("Fp2");
        let o1 = h("O1");
        let cz = h("Cz");
        assert!(fp1[0] < 0.0, "Fp1 must be left −X, got {}", fp1[0]);
        assert!(fp2[0] > 0.0, "Fp2 must be right +X, got {}", fp2[0]);
        assert!(fp1[1] < 0.0, "Fp anterior is −Y, got {}", fp1[1]);
        assert!(o1[1] > 0.0, "O1 posterior is +Y, got {}", o1[1]);
        assert!(cz[2] > 0.35, "Cz still up, got {}", cz[2]);
        assert!(
            cz[2] > cz[0].abs() && cz[2] > cz[1].abs(),
            "Cz must be +Z, got {cz:?}"
        );
        let zmax = mesh().holes.iter().map(|h| h.p[2]).fold(f32::MIN, f32::max);
        assert!(
            (cz[2] - zmax).abs() < 1e-5,
            "Cz must be the highest insert, cz={} zmax={}",
            cz[2],
            zmax
        );
    }

    #[test]
    fn thirty_five_inserts_sit_on_circular_rims_not_lattice_or_096() {
        let m = mesh();
        assert_eq!(m.holes.len(), 35);
        let names: Vec<&str> = m.holes.iter().map(|h| h.name.as_str()).collect();
        for need in [
            "Fp1", "Fp2", "F7", "F8", "C3", "C4", "P3", "P4", "O1", "O2", "Cz", "Fpz",
        ] {
            assert!(names.contains(&need), "missing {need} in {names:?}");
        }
        let rs: Vec<(&str, f32)> = m
            .holes
            .iter()
            .map(|h| (h.name.as_str(), len3(h.p)))
            .collect();
        let all_096 = rs.iter().all(|(_, r)| (*r - 0.960).abs() < 0.03);
        assert!(
            !all_096,
            "names still on the ideal r=0.96 sphere (decorative-looking snap): {rs:?}"
        );
        for h in &m.holes {
            assert!(
                sits_on_circular_rim(&m.verts, h.p),
                "{} at {:?} is off a circular INSERT rim (lattice snap or r=0.96)",
                h.name,
                h.p
            );
        }
        // A mesh vert on a lattice rib (many neighbors, not an empty circle) must fail.
        let mut decorative = None;
        for v in &m.verts {
            if len3(*v) < 0.5 {
                continue;
            }
            let far = m.holes.iter().all(|h| len3(sub3(*v, h.p)) > 0.16);
            if !far {
                continue;
            }
            if !sits_on_circular_rim(&m.verts, *v) {
                decorative = Some(*v);
                break;
            }
        }
        assert!(
            decorative.is_some(),
            "expected a decorative lattice vert that is not an insert rim"
        );
        let d = decorative.unwrap();
        assert!(
            !sits_on_circular_rim(&m.verts, d),
            "decorative lattice vert {d:?} must not count as an insert"
        );
    }

    #[test]
    fn default_8_occupied_mark_iv_cyton() {
        let occ = default_map();
        assert_eq!(
            occ.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            vec!["Fp1", "Fp2", "C3", "C4", "P7", "P8", "O1", "O2"]
        );
        assert!(channel_at(&occ, "C3").is_some());
        assert!(channel_at(&occ, "O1").is_some());
        assert!(channel_at(&occ, "O2").is_some());
        assert!(channel_at(&occ, "P7").is_some());
        assert!(channel_at(&occ, "P8").is_some());
        assert!(channel_at(&occ, "F7").is_none());
        assert!(channel_at(&occ, "F8").is_none());
        assert!(channel_at(&occ, "P3").is_none());
        assert!(channel_at(&occ, "P4").is_none());
    }

    #[test]
    fn three_quarter_view_reads_headset_and_holes() {
        let cam = Camera::THREE_QUARTER;
        let c = Pos2::new(200.0, 200.0);
        let s = 140.0;
        let xy = |n: &str| {
            let p = mesh().holes.iter().find(|h| h.name == n).unwrap().p;
            project(p, cam, c, s)
        };
        let f7 = xy("F7");
        let f8 = xy("F8");
        let fp1 = xy("Fp1");
        let p3 = xy("P3");
        let fp2 = xy("Fp2");
        let c3 = xy("C3");
        let c4 = xy("C4");
        let p4 = xy("P4");
        assert!(
            f7.pos.x < f8.pos.x,
            "F7 left of F8 in 3/4: {} vs {}",
            f7.pos.x,
            f8.pos.x
        );
        let cz = xy("Cz");
        let neck = project([0.0, 0.0, -0.7], cam, c, s);
        assert!(
            cz.pos.y < neck.pos.y,
            "crown (Cz) above chin/neck (egui y-down): cz={} neck={}",
            cz.pos.y,
            neck.pos.y
        );
        for (n, pr) in [
            ("Fp1", fp1),
            ("Fp2", fp2),
            ("F7", f7),
            ("F8", f8),
            ("C3", c3),
            ("C4", c4),
            ("P3", p3),
            ("P4", p4),
        ] {
            assert!(
                pr.pos.x > 20.0 && pr.pos.x < 380.0 && pr.pos.y > 20.0 && pr.pos.y < 380.0,
                "{n} off-frame {:?}",
                pr.pos
            );
        }
        assert!(
            (f8.pos.x - f7.pos.x).abs() > 20.0,
            "3/4 must separate hemispheres"
        );
    }

    #[test]
    fn project_up_is_up() {
        let cam = Camera::THREE_QUARTER;
        let c = Pos2::new(200.0, 200.0);
        let s = 140.0;
        let crown = project([0.0, 0.0, 0.9], cam, c, s);
        let neck = project([0.0, 0.0, -0.7], cam, c, s);
        assert!(
            crown.pos.y < neck.pos.y,
            "world +Z must project to smaller screen y, crown={} neck={}",
            crown.pos.y,
            neck.pos.y
        );
    }

    #[test]
    fn default_camera_is_three_quarter() {
        assert!(Camera::THREE_QUARTER.yaw == VIEW_YAW);
        assert_eq!(Camera::THREE_QUARTER.pitch, VIEW_PITCH);
        let mut c = Camera::default();
        let y0 = c.yaw;
        c.drag(Vec2::new(20.0, 0.0));
        assert!((c.yaw - y0).abs() > 0.1);
    }

    #[test]
    fn pair_view_frame_skips_geodesic_strut_between_labeled_inserts() {
        let skip = pair_strut_skip_indices("P3", "P4");
        assert!(
            !skip.is_empty(),
            "P3–P4 geodesic must drop the lattice strut, skipped={}",
            skip.len()
        );
        assert!(
            skip.len() < mesh().faces.len() / 8,
            "must not drop the whole lattice, skipped={}",
            skip.len()
        );
        let m = mesh();
        let p3 = hole_pos("P3").unwrap();
        let p4 = hole_pos("P4").unwrap();
        // Pair-view paint must not emit a face whose both ends are the two holes.
        for (i, face) in m.faces.iter().enumerate() {
            if skip.contains(&i) {
                continue;
            }
            let vs = [
                m.verts[face[0] as usize],
                m.verts[face[1] as usize],
                m.verts[face[2] as usize],
            ];
            let near_a = vs.iter().any(|v| dist3(*v, p3) < PAIR_STRUT_END);
            let near_b = vs.iter().any(|v| dist3(*v, p4) < PAIR_STRUT_END);
            assert!(
                !(near_a && near_b),
                "emitted face {i} still spans P3 and P4"
            );
        }
        let skip_c = pair_strut_skip_indices("C3", "C4");
        assert!(
            !skip_c.is_empty(),
            "C3–C4 geodesic must also drop a strut"
        );
        // Head Plot uses paint_frame (no skip). Unknown names skip nothing.
        assert!(pair_strut_skip_indices("P3", "nope").is_empty());
    }

    #[test]
    fn pair_view_screen_skip_drops_p3_p4_segment() {
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(400.0, 400.0));
        let cam = Camera::default();
        let geo = pair_strut_skip_indices("P3", "P4");
        let screen = pair_screen_skip_indices("P3", "P4", rect, cam);
        assert!(
            !screen.is_empty(),
            "P3–P4 screen corridor must drop remaining ridge, skipped={}",
            screen.len()
        );
        assert!(
            screen.len() < mesh().faces.len() / 4,
            "must not drop the whole lattice, skipped={}",
            screen.len()
        );
        let extra = screen.difference(&geo).count();
        assert!(
            extra > 0,
            "screen skip must catch geodesic-missed ridge faces, extra={extra}"
        );
        let m = mesh();
        let center = rect.center();
        let scale = canvas_scale(rect);
        let p3 = hole_pos("P3").unwrap();
        let p4 = hole_pos("P4").unwrap();
        let a2 = project(p3, cam, center, scale).pos;
        let b2 = project(p4, cam, center, scale).pos;
        let mut union = geo.clone();
        union.extend(screen.iter().copied());
        for (i, face) in m.faces.iter().enumerate() {
            if union.contains(&i) {
                continue;
            }
            let c3 = face_centroid(m, *face);
            if dist3(c3, p3) < PAIR_STRUT_RIM || dist3(c3, p4) < PAIR_STRUT_RIM {
                continue;
            }
            let ca = project(m.verts[face[0] as usize], cam, center, scale).pos;
            let cb = project(m.verts[face[1] as usize], cam, center, scale).pos;
            let cc = project(m.verts[face[2] as usize], cam, center, scale).pos;
            let d = dist_tri_seg2(ca, cb, cc, a2, b2);
            assert!(
                d > PAIR_SCREEN_PX,
                "emitted face {i} still on P3–P4 screen segment d={d}"
            );
        }
        assert!(pair_screen_skip_indices("P3", "nope", rect, cam).is_empty());
    }
}
