//! Ultracortex Mark IV frame + 10-20 holes.
//! Medium print STLs live in resources/ultracortex_mark_iv/ (OpenBCI Docs STL_Directory).
//! Full meshes are ~50k tris each; Head Plot draws a lit orbit of a simplified frame
//! with the same 10-20 node layout.

use std::sync::OnceLock;

pub const HEADSET_MARK_IV: &str = "Ultracortex Mark IV";
pub const STL_DIR_REL: &str = "resources/ultracortex_mark_iv";
pub const STL_FRONT: &str = "M4_Medium_Front.stl";
pub const STL_BACK: &str = "M4_Medium_Back.stl";

/// Official Ultracortex Mark IV Cyton 8ch (docs.openbci.com).
pub const DEFAULT_SITES: [&str; 8] = ["Fp1", "Fp2", "C3", "C4", "P7", "P8", "O1", "O2"];

/// Classic 10-20 holes present on a Mark IV medium frame.
pub const HOLES: [&str; 35] = [
    "Fp1", "Fp2", "Fpz", "F7", "F3", "Fz", "F4", "F8", "T7", "C3", "Cz", "C4", "T8", "P7", "P3",
    "Pz", "P4", "P8", "O1", "Oz", "O2", "AF3", "AF4", "FT7", "FC3", "FCz", "FC4", "FT8", "TP7",
    "CP3", "CPz", "CP4", "TP8", "PO3", "PO4",
];

const STRUTS: [(&str, &str); 40] = [
    ("Fp1", "Fp2"),
    ("Fp1", "Fpz"),
    ("Fp2", "Fpz"),
    ("Fpz", "Fz"),
    ("Fp1", "F3"),
    ("Fp2", "F4"),
    ("Fp1", "F7"),
    ("Fp2", "F8"),
    ("F7", "F3"),
    ("F3", "Fz"),
    ("Fz", "F4"),
    ("F4", "F8"),
    ("F7", "T7"),
    ("F8", "T8"),
    ("F3", "C3"),
    ("Fz", "Cz"),
    ("F4", "C4"),
    ("T7", "C3"),
    ("C3", "Cz"),
    ("Cz", "C4"),
    ("C4", "T8"),
    ("T7", "P7"),
    ("C3", "P3"),
    ("Cz", "Pz"),
    ("C4", "P4"),
    ("T8", "P8"),
    ("P7", "P3"),
    ("P3", "Pz"),
    ("Pz", "P4"),
    ("P4", "P8"),
    ("P7", "O1"),
    ("P3", "O1"),
    ("Pz", "Oz"),
    ("P4", "O2"),
    ("P8", "O2"),
    ("O1", "Oz"),
    ("Oz", "O2"),
    ("F7", "C3"),
    ("F8", "C4"),
    ("O1", "O2"),
];

#[derive(Clone, Copy, Debug)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl V3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    pub fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Self {
        let l = self.length().max(1e-8);
        self.mul(1.0 / l)
    }
}

/// Spherical 10-20: θ from +Z (Cz), φ from +Y (nasion) toward +X (right).
pub fn sph(theta_deg: f32, phi_deg: f32) -> V3 {
    let th = theta_deg.to_radians();
    let ph = phi_deg.to_radians();
    V3::new(th.sin() * ph.sin(), th.sin() * ph.cos(), th.cos())
}

pub fn hole_xyz(name: &str) -> Option<V3> {
    let p = match name {
        "Cz" => sph(0.0, 0.0),
        "Fz" => sph(42.0, 0.0),
        "Pz" => sph(42.0, 180.0),
        "C3" => sph(45.0, -90.0),
        "C4" => sph(45.0, 90.0),
        "T7" => sph(88.0, -90.0),
        "T8" => sph(88.0, 90.0),
        "Fpz" => sph(70.0, 0.0),
        "Oz" => sph(70.0, 180.0),
        "Fp1" => sph(70.0, -22.0),
        "Fp2" => sph(70.0, 22.0),
        "F7" => sph(72.0, -54.0),
        "F8" => sph(72.0, 54.0),
        "P7" => sph(72.0, -126.0),
        "P8" => sph(72.0, 126.0),
        "O1" => sph(70.0, -158.0),
        "O2" => sph(70.0, 158.0),
        "F3" => sph(48.0, -40.0),
        "F4" => sph(48.0, 40.0),
        "P3" => sph(48.0, -140.0),
        "P4" => sph(48.0, 140.0),
        "AF3" => sph(62.0, -28.0),
        "AF4" => sph(62.0, 28.0),
        "FT7" => sph(80.0, -70.0),
        "FT8" => sph(80.0, 70.0),
        "FC3" => sph(50.0, -50.0),
        "FCz" => sph(28.0, 0.0),
        "FC4" => sph(50.0, 50.0),
        "TP7" => sph(80.0, -110.0),
        "TP8" => sph(80.0, 110.0),
        "CP3" => sph(50.0, -130.0),
        "CPz" => sph(28.0, 180.0),
        "CP4" => sph(50.0, 130.0),
        "PO3" => sph(62.0, -152.0),
        "PO4" => sph(62.0, 152.0),
        _ => return None,
    };
    Some(p)
}

pub fn is_hole(name: &str) -> bool {
    HOLES.iter().any(|h| *h == name)
}

pub fn default_map() -> [String; 8] {
    DEFAULT_SITES.map(|s| s.to_string())
}

pub fn channel_at<'a>(map: &'a [String; 8], site: &str) -> Option<usize> {
    map.iter().position(|s| s == site)
}

/// Camera: yaw about +Z, pitch from front (+Y) toward top-down (+Z).
/// Returns camera-space (x_right, y_up, depth_forward).
pub fn orbit_point(p: V3, yaw: f32, pitch: f32) -> V3 {
    let (sy, cy) = (yaw.sin(), yaw.cos());
    let x1 = p.x * cy - p.y * sy;
    let y1 = p.x * sy + p.y * cy;
    let z1 = p.z;
    let (sp, cp) = (pitch.sin(), pitch.cos());
    let depth = y1 * cp + z1 * sp;
    let up = -y1 * sp + z1 * cp;
    V3::new(x1, up, depth)
}

pub fn project_to_px(p: V3, yaw: f32, pitch: f32, w: f32, h: f32) -> (f32, f32, f32) {
    let c = orbit_point(p, yaw, pitch);
    let dist = 2.55;
    let s = (w.min(h) * 0.42) / (dist + c.z).max(0.35);
    let cx = w * 0.5;
    let cy = h * 0.52;
    (cx + c.x * s, cy - c.y * s, c.z)
}

#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub a: V3,
    pub b: V3,
    pub c: V3,
    pub n: V3,
    pub rgb: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct TriMesh {
    pub tris: Vec<Tri>,
}

impl TriMesh {
    fn push(&mut self, a: V3, b: V3, c: V3, rgb: [f32; 3]) {
        let n = b.sub(a).cross(c.sub(a)).normalized();
        self.tris.push(Tri { a, b, c, n, rgb });
    }
}

fn add_cylinder(mesh: &mut TriMesh, a: V3, b: V3, radius: f32, segs: usize, rgb: [f32; 3]) {
    let axis = b.sub(a);
    let len = axis.length();
    if len < 1e-4 {
        return;
    }
    let axis_n = axis.mul(1.0 / len);
    let helper = if axis_n.z.abs() < 0.9 {
        V3::new(0.0, 0.0, 1.0)
    } else {
        V3::new(0.0, 1.0, 0.0)
    };
    let u = axis_n.cross(helper).normalized();
    let v = axis_n.cross(u).normalized();
    let tau = std::f32::consts::TAU;
    for i in 0..segs {
        let a0 = (i as f32) * tau / (segs as f32);
        let a1 = ((i + 1) as f32) * tau / (segs as f32);
        let r0 = u.mul(a0.cos() * radius).add(v.mul(a0.sin() * radius));
        let r1 = u.mul(a1.cos() * radius).add(v.mul(a1.sin() * radius));
        mesh.push(a.add(r0), b.add(r0), b.add(r1), rgb);
        mesh.push(a.add(r0), b.add(r1), a.add(r1), rgb);
    }
}

fn add_torus(
    mesh: &mut TriMesh,
    center: V3,
    normal: V3,
    major: f32,
    minor: f32,
    segs_u: usize,
    segs_v: usize,
    rgb: [f32; 3],
) {
    let n = normal.normalized();
    let helper = if n.z.abs() < 0.9 {
        V3::new(0.0, 0.0, 1.0)
    } else {
        V3::new(0.0, 1.0, 0.0)
    };
    let u = n.cross(helper).normalized();
    let v = n.cross(u).normalized();
    let tau = std::f32::consts::TAU;
    for i in 0..segs_u {
        let t0 = (i as f32) * tau / (segs_u as f32);
        let t1 = ((i + 1) as f32) * tau / (segs_u as f32);
        let ring0 = u.mul(t0.cos()).add(v.mul(t0.sin()));
        let ring1 = u.mul(t1.cos()).add(v.mul(t1.sin()));
        let c0 = center.add(ring0.mul(major));
        let c1 = center.add(ring1.mul(major));
        for j in 0..segs_v {
            let p0 = (j as f32) * tau / (segs_v as f32);
            let p1 = ((j + 1) as f32) * tau / (segs_v as f32);
            let q00 = c0.add(ring0.mul(p0.cos() * minor).add(n.mul(p0.sin() * minor)));
            let q01 = c0.add(ring0.mul(p1.cos() * minor).add(n.mul(p1.sin() * minor)));
            let q10 = c1.add(ring1.mul(p0.cos() * minor).add(n.mul(p0.sin() * minor)));
            let q11 = c1.add(ring1.mul(p1.cos() * minor).add(n.mul(p1.sin() * minor)));
            mesh.push(q00, q10, q11, rgb);
            mesh.push(q00, q11, q01, rgb);
        }
    }
}

fn add_box(mesh: &mut TriMesh, center: V3, right: V3, up: V3, fwd: V3, rgb: [f32; 3]) {
    let corners = |sx: f32, sy: f32, sz: f32| {
        center
            .add(right.mul(sx))
            .add(up.mul(sy))
            .add(fwd.mul(sz))
    };
    let p = [
        corners(-1.0, -1.0, -1.0),
        corners(1.0, -1.0, -1.0),
        corners(1.0, 1.0, -1.0),
        corners(-1.0, 1.0, -1.0),
        corners(-1.0, -1.0, 1.0),
        corners(1.0, -1.0, 1.0),
        corners(1.0, 1.0, 1.0),
        corners(-1.0, 1.0, 1.0),
    ];
    let faces = [
        [0, 1, 2, 3],
        [4, 7, 6, 5],
        [0, 4, 5, 1],
        [3, 2, 6, 7],
        [0, 3, 7, 4],
        [1, 5, 6, 2],
    ];
    for f in faces {
        mesh.push(p[f[0]], p[f[1]], p[f[2]], rgb);
        mesh.push(p[f[0]], p[f[2]], p[f[3]], rgb);
    }
}

const FRAME_BIN: &[u8] = include_bytes!("../resources/ultracortex_mark_iv/frame.bin");
const PLASTIC: [f32; 3] = [0.40, 0.40, 0.43];

fn load_frame_bin(bytes: &[u8]) -> Option<TriMesh> {
    if bytes.len() < 16 || &bytes[0..4] != b"M4FR" {
        return None;
    }
    let mut off = 4usize;
    let u32le = |off: &mut usize| -> Option<u32> {
        let s = *off;
        if s + 4 > bytes.len() {
            return None;
        }
        *off += 4;
        Some(u32::from_le_bytes(bytes[s..s + 4].try_into().ok()?))
    };
    let f32le = |off: &mut usize| -> Option<f32> {
        let s = *off;
        if s + 4 > bytes.len() {
            return None;
        }
        *off += 4;
        Some(f32::from_le_bytes(bytes[s..s + 4].try_into().ok()?))
    };
    let ver = u32le(&mut off)?;
    if ver != 1 {
        return None;
    }
    let nverts = u32le(&mut off)? as usize;
    let nfaces = u32le(&mut off)? as usize;
    let mut verts = Vec::with_capacity(nverts);
    for _ in 0..nverts {
        let x = f32le(&mut off)?;
        let y = f32le(&mut off)?;
        let z = f32le(&mut off)?;
        // frame.bin: +X right, −Y anterior. headset3d: +Y anterior.
        verts.push(V3::new(x, -y, z));
    }
    // skip face normals
    off = off.saturating_add(nfaces.saturating_mul(12));
    if off + nfaces * 12 > bytes.len() {
        return None;
    }
    let mut mesh = TriMesh::default();
    for _ in 0..nfaces {
        let i0 = u32le(&mut off)? as usize;
        let i1 = u32le(&mut off)? as usize;
        let i2 = u32le(&mut off)? as usize;
        if i0 >= nverts || i1 >= nverts || i2 >= nverts {
            continue;
        }
        mesh.push(verts[i0], verts[i1], verts[i2], PLASTIC);
    }
    if mesh.tris.len() < 200 {
        return None;
    }
    Some(mesh)
}

pub fn simplified_mark_iv() -> TriMesh {
    let plastic = [0.40, 0.40, 0.43];
    let collar = [0.48, 0.48, 0.50];
    let board = [0.18, 0.18, 0.20];
    let mut mesh = TriMesh::default();
    for name in HOLES {
        let Some(p) = hole_xyz(name) else {
            continue;
        };
        add_torus(&mut mesh, p, p, 0.072, 0.018, 10, 6, collar);
    }
    for (a, b) in STRUTS {
        let Some(pa) = hole_xyz(a) else {
            continue;
        };
        let Some(pb) = hole_xyz(b) else {
            continue;
        };
        let dir = pb.sub(pa).normalized();
        let a2 = pa.add(dir.mul(0.065));
        let b2 = pb.sub(dir.mul(0.065));
        add_cylinder(&mut mesh, a2, b2, 0.020, 8, plastic);
    }
    let c = sph(28.0, 180.0).mul(1.04);
    let right = V3::new(0.16, 0.0, 0.0);
    let up = V3::new(0.0, 0.0, 0.035);
    let fwd = V3::new(0.0, -0.11, 0.0);
    add_box(&mut mesh, c, right, up, fwd, board);
    mesh
}

pub fn frame_mesh() -> &'static TriMesh {
    static MESH: OnceLock<TriMesh> = OnceLock::new();
    MESH.get_or_init(|| load_frame_bin(FRAME_BIN).unwrap_or_else(simplified_mark_iv))
}

const LIGHT: V3 = V3::new(0.45, 0.55, 0.70);

fn shade(n: V3, yaw: f32, pitch: f32, rgb: [f32; 3]) -> [u8; 3] {
    let n = orbit_point(n, yaw, pitch).normalized();
    let l = LIGHT.normalized();
    let ndotl = n.dot(l).max(0.0);
    let view = V3::new(0.0, 0.0, 1.0);
    let half = l.add(view).normalized();
    let spec = n.dot(half).max(0.0).powi(20) * 0.22;
    let amb = 0.16;
    let mut out = [0u8; 3];
    for i in 0..3 {
        let v = (rgb[i] * (amb + 0.84 * ndotl) + spec).clamp(0.0, 1.0);
        out[i] = (v * 255.0) as u8;
    }
    out
}

/// Software z-buffer into RGBA (premultiplied-ish opaque on canvas).
pub fn rasterize(mesh: &TriMesh, w: usize, h: usize, yaw: f32, pitch: f32) -> Vec<u8> {
    let mut rgba = vec![0u8; w * h * 4];
    let bg = [0x1d_u8, 0x1d, 0x1d];
    for px in rgba.chunks_exact_mut(4) {
        px[0] = bg[0];
        px[1] = bg[1];
        px[2] = bg[2];
        px[3] = 255;
    }
    let mut zbuf = vec![f32::INFINITY; w * h];
    let wf = w as f32;
    let hf = h as f32;
    for t in &mesh.tris {
        let n_cam = orbit_point(t.n, yaw, pitch);
        if n_cam.z < 0.0 {
            continue;
        }
        let (ax, ay, az) = project_to_px(t.a, yaw, pitch, wf, hf);
        let (bx, by, bz) = project_to_px(t.b, yaw, pitch, wf, hf);
        let (cx, cy, cz) = project_to_px(t.c, yaw, pitch, wf, hf);
        let area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
        if area.abs() < 0.25 {
            continue;
        }
        let col = shade(t.n, yaw, pitch, t.rgb);
        let minx = ax.min(bx).min(cx).floor().max(0.0) as i32;
        let maxx = ax.max(bx).max(cx).ceil().min(wf - 1.0) as i32;
        let miny = ay.min(by).min(cy).floor().max(0.0) as i32;
        let maxy = ay.max(by).max(cy).ceil().min(hf - 1.0) as i32;
        let inv = 1.0 / area;
        for y in miny..=maxy {
            let py = y as f32 + 0.5;
            for x in minx..=maxx {
                let px = x as f32 + 0.5;
                let w0 = ((bx - px) * (cy - py) - (by - py) * (cx - px)) * inv;
                let w1 = ((cx - px) * (ay - py) - (cy - py) * (ax - px)) * inv;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * az + w1 * bz + w2 * cz;
                let idx = (y as usize) * w + (x as usize);
                if z < zbuf[idx] {
                    zbuf[idx] = z;
                    let o = idx * 4;
                    rgba[o] = col[0];
                    rgba[o + 1] = col[1];
                    rgba[o + 2] = col[2];
                    rgba[o + 3] = 255;
                }
            }
        }
    }
    rgba
}

pub fn parse_binary_stl(bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() < 84 {
        return Err("stl too small".into());
    }
    if bytes.starts_with(b"solid") && !bytes[80..84].iter().any(|&b| b == 0) {
        return Err("ascii stl".into());
    }
    let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    let need = 84 + n * 50;
    if bytes.len() < need {
        return Err(format!("truncated stl: have {} need {need}", bytes.len()));
    }
    if n < 100 {
        return Err(format!("too few tris: {n}"));
    }
    Ok(n)
}

pub fn crate_stl_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(STL_DIR_REL)
}


pub fn paint_frame(painter: &eframe::egui::Painter, rect: eframe::egui::Rect, yaw: f32, pitch: f32) {
    use eframe::egui::{self, Color32, Mesh, Pos2};
    let mesh = frame_mesh();
    let w = rect.width();
    let h = rect.height();
    let mut tris: Vec<(f32, Color32, [Pos2; 3])> = Vec::with_capacity(mesh.tris.len());
    for t in &mesh.tris {
        let n_cam = orbit_point(t.n, yaw, pitch);
        if n_cam.z < -0.02 {
            continue;
        }
        let (ax, ay, az) = project_to_px(t.a, yaw, pitch, w, h);
        let (bx, by, bz) = project_to_px(t.b, yaw, pitch, w, h);
        let (cx, cy, cz) = project_to_px(t.c, yaw, pitch, w, h);
        let pa = rect.min + egui::vec2(ax, ay);
        let pb = rect.min + egui::vec2(bx, by);
        let pc = rect.min + egui::vec2(cx, cy);
        let col = shade(t.n, yaw, pitch, t.rgb);
        let color = Color32::from_rgb(col[0], col[1], col[2]);
        let depth = (az + bz + cz) / 3.0;
        tris.push((depth, color, [pa, pb, pc]));
    }
    tris.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut gpu = Mesh::default();
    for (_d, col, pts) in tris {
        let i0 = gpu.vertices.len() as u32;
        for p in pts {
            gpu.vertices.push(egui::epaint::Vertex {
                pos: p,
                uv: Pos2::ZERO,
                color: col,
            });
        }
        gpu.indices.extend_from_slice(&[i0, i0 + 1, i0 + 2]);
    }
    painter.add(egui::Shape::mesh(gpu));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_iv_holes_include_default_and_o1_o2() {
        assert_eq!(HOLES.len(), 35);
        for s in DEFAULT_SITES {
            assert!(HOLES.contains(&s), "missing {s}");
        }
        assert!(HOLES.contains(&"O1"));
        assert!(HOLES.contains(&"O2"));
        assert_eq!(channel_at(&default_map(), "O1"), Some(6));
        assert_eq!(channel_at(&default_map(), "O2"), Some(7));
        assert!(channel_at(&default_map(), "F7").is_none());
        assert!(channel_at(&default_map(), "P3").is_none());
        assert_eq!(channel_at(&default_map(), "C3"), Some(2));
        assert_eq!(channel_at(&default_map(), "P7"), Some(4));
        assert_eq!(default_map(), DEFAULT_SITES.map(|s| s.to_string()));
    }

    #[test]
    fn default_view_is_3d_not_topdown_oval() {
        let yaw = 0.42;
        let pitch = 0.72;
        let (fx, fy, _) = project_to_px(hole_xyz("Fpz").unwrap(), yaw, pitch, 400.0, 400.0);
        let (ox, oy, _) = project_to_px(hole_xyz("Oz").unwrap(), yaw, pitch, 400.0, 400.0);
        let (l7x, _, _) = project_to_px(hole_xyz("F7").unwrap(), yaw, pitch, 400.0, 400.0);
        let (r8x, _, _) = project_to_px(hole_xyz("F8").unwrap(), yaw, pitch, 400.0, 400.0);
        assert!(fy < oy, "anterior should sit above posterior: Fpz={fy} Oz={oy}");
        assert!(l7x < r8x, "F7 left of F8: {l7x} vs {r8x}");
        assert!((fx - ox).abs() > 8.0, "front/back must separate in x under yaw, dx={}", (fx-ox).abs());
    }

    #[test]
    fn simplified_frame_has_tris() {
        let m = simplified_mark_iv();
        assert!(m.tris.len() > 400, "got {}", m.tris.len());
        assert!(m.tris.len() < 12_000, "too heavy {}", m.tris.len());
    }

    #[test]
    fn vendored_frame_bin_is_official_decimated_mark_iv() {
        let m = load_frame_bin(FRAME_BIN).expect("frame.bin");
        assert!(m.tris.len() > 1000, "got {}", m.tris.len());
        assert!(m.tris.len() < 20_000, "too heavy {}", m.tris.len());
        let src = include_str!("../resources/ultracortex_mark_iv/SOURCE.md");
        assert!(src.contains("M4_Medium_Front.stl"));
        assert!(src.contains("M4_Medium_Back.stl"));
        assert!(src.contains("OpenBCI"));
    }

    #[test]
    fn raster_paints_non_canvas() {
        let m = frame_mesh();
        let img = rasterize(m, 80, 80, 0.42, 0.72);
        let mut lit = 0u32;
        for px in img.chunks_exact(4) {
            if px[0] > 0x28 || px[1] > 0x28 || px[2] > 0x28 {
                lit += 1;
            }
        }
        assert!(lit > 80, "expected a visible headset, lit={lit}");
    }
}
