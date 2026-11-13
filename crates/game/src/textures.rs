//! Procedural textures (no image files): brick, concrete, court wood, fabric weave, skin
//! and a pebbled ball grip, each with a matching normal map for shading. All tile
//! seamlessly and carry full mip chains so they stay smooth in the distance. Also a box
//! mesh whose UVs follow world size, so textures tile at a constant scale on any wall.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// A colour texture and its normal map.
#[derive(Clone)]
pub struct Tex {
    pub color: Handle<Image>,
    pub normal: Handle<Image>,
}

/// Every procedural texture, made once at startup.
#[derive(Resource, Clone)]
pub struct Textures {
    pub brick: Tex,
    pub concrete: Tex,
    pub court: Tex,
    pub fabric: Tex,
    pub skin: Tex,
    pub ball: Tex,
    pub panel: Tex,
}

pub struct TexturesPlugin;

impl Plugin for TexturesPlugin {
    fn build(&self, app: &mut App) {
        // Runs before anything else in Startup that needs the textures.
        app.add_systems(PreStartup, make_textures);
    }
}

fn make_textures(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut tex = |size: u32, strength: f32, f: &dyn Fn(f32, f32) -> ([f32; 3], f32)| {
        let (c, n) = bake(size, strength, f);
        Tex { color: images.add(c), normal: images.add(n) }
    };
    let textures = Textures {
        brick: tex(256, 3.0, &brick),
        concrete: tex(256, 1.5, &concrete),
        court: tex(256, 1.2, &court),
        fabric: tex(128, 1.6, &fabric),
        skin: tex(128, 0.5, &skin),
        ball: tex(256, 2.5, &ball_grip),
        panel: tex(256, 2.0, &panel),
    };
    commands.insert_resource(textures);
}

// ---------------------------------------------------------------- noise helpers

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

/// Smooth value noise that repeats every `period` cells over the unit square.
fn noise(u: f32, v: f32, period: i32, seed: u32) -> f32 {
    let (x, y) = (u * period as f32, v * period as f32);
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let at = |i: i32, j: i32| hash(i.rem_euclid(period), j.rem_euclid(period), seed);
    let a = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * s(fx);
    let b = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * s(fx);
    a + (b - a) * s(fy)
}

/// A few octaves of tileable noise, 0..1.
fn fbm(u: f32, v: f32, period: i32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut p = period;
    for o in 0..4 {
        sum += noise(u, v, p, seed + o) * amp;
        amp *= 0.5;
        p *= 2;
    }
    sum / 0.9375
}

fn scale(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

// ---------------------------------------------------------------- patterns
// Each returns (linear-ish colour 0..1, height 0..1). Colours are near white / grey so
// materials can tint them with `base_color`.

/// Running-bond bricks with recessed mortar and per-brick tone.
fn brick(u: f32, v: f32) -> ([f32; 3], f32) {
    const ROWS: f32 = 8.0;
    const COLS: f32 = 4.0;
    let row = (v * ROWS).floor();
    let shift = if row as i32 % 2 == 0 { 0.0 } else { 0.5 };
    let x = u * COLS + shift;
    let col = x.floor();
    let (fx, fy) = (x.fract(), (v * ROWS).fract());
    let edge = fx.min(1.0 - fx) * 2.0 / COLS * ROWS * 0.5;
    let edge = edge.min(fy.min(1.0 - fy));
    let mortar = 0.07;
    let grain = fbm(u, v, 16, 7);
    let tone = 0.82 + 0.3 * hash(col as i32, row as i32, 3);
    if edge < mortar {
        ([0.72, 0.70, 0.66], 0.15 + 0.1 * grain)
    } else {
        let bevel = ((edge - mortar) / 0.06).min(1.0);
        let c = scale([1.0, 0.92, 0.86], tone * (0.85 + 0.25 * grain));
        (c, 0.5 + 0.45 * bevel + 0.08 * grain)
    }
}

/// Poured concrete: soft blotches, fine grit and a few pits.
fn concrete(u: f32, v: f32) -> ([f32; 3], f32) {
    let blotch = fbm(u, v, 4, 11);
    let grit = noise(u, v, 64, 12);
    let pit = if hash((u * 128.0) as i32, (v * 128.0) as i32, 13) > 0.985 { 0.0 } else { 1.0 };
    let k = 0.78 + 0.25 * blotch + 0.08 * grit;
    (scale([1.0, 1.0, 0.98], k * (0.85 + 0.15 * pit)), 0.4 * blotch + 0.3 * grit + 0.3 * pit)
}

/// Court floor: long boards with grain, alternating tone, seams between them.
fn court(u: f32, v: f32) -> ([f32; 3], f32) {
    const BOARDS: f32 = 8.0;
    let b = (u * BOARDS).floor();
    let fx = (u * BOARDS).fract();
    // Board ends staggered along v.
    let offset = hash(b as i32, 0, 21);
    let fv = (v * 2.0 + offset).fract();
    let seam = fx.min(1.0 - fx) < 0.035 || fv.min(1.0 - fv) < 0.012;
    let grain = noise(u * 0.25, v * 4.0, 32, 22) * 0.6 + noise(u, v, 64, 23) * 0.4;
    let tone = 0.85 + 0.2 * hash(b as i32, (v * 2.0 + offset).floor() as i32, 24);
    let c = scale([1.0, 0.97, 0.92], tone * (0.88 + 0.2 * grain));
    if seam {
        (scale(c, 0.6), 0.1)
    } else {
        (c, 0.6 + 0.2 * grain)
    }
}

/// Woven cloth: over-under threads.
fn fabric(u: f32, v: f32) -> ([f32; 3], f32) {
    const THREADS: f32 = 32.0;
    let (x, y) = (u * THREADS, v * THREADS);
    let warp = (x * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let weft = (y * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let over = if (x.floor() + y.floor()) as i32 % 2 == 0 { warp } else { weft };
    let fuzz = noise(u, v, 32, 31);
    (scale([1.0, 1.0, 1.0], 0.85 + 0.15 * over + 0.05 * fuzz), over * 0.8 + 0.2 * fuzz)
}

/// Skin: very soft blotches and pores.
fn skin(u: f32, v: f32) -> ([f32; 3], f32) {
    let soft = fbm(u, v, 4, 41);
    let pore = noise(u, v, 64, 42);
    (scale([1.0, 0.97, 0.96], 0.94 + 0.08 * soft), 0.5 + 0.3 * pore + 0.2 * soft)
}

/// Playground-ball grip: rows of tiny pebbles, with seam grooves on the equator and two
/// meridians (UV-sphere mapping: u around, v pole to pole).
fn ball_grip(u: f32, v: f32) -> ([f32; 3], f32) {
    const CELLS: f32 = 48.0;
    let (x, y) = (u * CELLS, v * CELLS * 0.5);
    let (fx, fy) = (x.fract() - 0.5, y.fract() - 0.5);
    let r = (fx * fx + fy * fy).sqrt();
    let pebble = (1.0 - r * 2.2).max(0.0);
    let groove = |d: f32| (d / 0.008).min(1.0);
    let seam = groove((v - 0.5).abs()) * groove((u - 0.25).abs().min((u - 0.75).abs()));
    let k = 0.9 + 0.1 * pebble;
    let c = if seam < 1.0 { scale([1.0, 1.0, 1.0], 0.45 + 0.4 * seam) } else { scale([1.0, 1.0, 1.0], k) };
    (c, (0.35 + 0.65 * pebble) * seam)
}

/// Painted metal panels: big plates with seams and rivets (parkour panels, frames).
fn panel(u: f32, v: f32) -> ([f32; 3], f32) {
    let (x, y) = ((u * 2.0).fract(), (v * 2.0).fract());
    let seam = x.min(1.0 - x) < 0.015 || y.min(1.0 - y) < 0.015;
    let rx = x.min(1.0 - x);
    let ry = y.min(1.0 - y);
    let rivet = ((rx - 0.05).powi(2) + (ry - 0.05).powi(2)).sqrt() < 0.018;
    let wear = fbm(u, v, 8, 51);
    let c = scale([1.0, 1.0, 1.0], 0.88 + 0.14 * wear);
    if seam {
        (scale(c, 0.55), 0.0)
    } else if rivet {
        (scale(c, 1.1), 1.0)
    } else {
        (c, 0.5 + 0.1 * wear)
    }
}

// ---------------------------------------------------------------- baking

/// Sample `f` over a `size` square and build an sRGB colour image and a normal map (from
/// the height channel, `strength` = bumpiness), both with full mip chains.
fn bake(size: u32, strength: f32, f: &dyn Fn(f32, f32) -> ([f32; 3], f32)) -> (Image, Image) {
    let n = size as usize;
    let mut color = vec![[0.0f32; 4]; n * n];
    let mut height = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let (c, h) = f((x as f32 + 0.5) / size as f32, (y as f32 + 0.5) / size as f32);
            color[y * n + x] = [c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0), 1.0];
            height[y * n + x] = h;
        }
    }
    let at = |x: isize, y: isize| height[(y.rem_euclid(n as isize) as usize) * n + x.rem_euclid(n as isize) as usize];
    let mut normal = vec![[0.0f32; 4]; n * n];
    for y in 0..n as isize {
        for x in 0..n as isize {
            let dx = (at(x + 1, y) - at(x - 1, y)) * strength;
            let dy = (at(x, y + 1) - at(x, y - 1)) * strength;
            let v = Vec3::new(-dx, -dy, 1.0).normalize();
            normal[y as usize * n + x as usize] = [v.x * 0.5 + 0.5, v.y * 0.5 + 0.5, v.z * 0.5 + 0.5, 1.0];
        }
    }
    (to_image(size, color, true), to_image(size, normal, false))
}

/// Pack RGBA floats (and a box-filtered mip chain) into an image that tiles.
fn to_image(size: u32, base: Vec<[f32; 4]>, srgb: bool) -> Image {
    let encode = |c: f32| -> u8 {
        let c = c.clamp(0.0, 1.0);
        let c = if srgb { if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 } } else { c };
        (c * 255.0 + 0.5) as u8
    };
    let mut data = Vec::new();
    let mut level = base;
    let mut s = size as usize;
    let mut levels = 0;
    loop {
        for p in &level {
            data.extend(p.iter().map(|&c| encode(c)));
        }
        levels += 1;
        if s == 1 {
            break;
        }
        let half = s / 2;
        let mut next = vec![[0.0f32; 4]; half * half];
        for y in 0..half {
            for x in 0..half {
                let mut sum = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = level[(y * 2 + dy) * s + x * 2 + dx];
                    for k in 0..4 {
                        sum[k] += p[k] * 0.25;
                    }
                }
                if !srgb {
                    // Keep averaged normals unit length.
                    let v = Vec3::new(sum[0] * 2.0 - 1.0, sum[1] * 2.0 - 1.0, sum[2] * 2.0 - 1.0).normalize_or(Vec3::Z);
                    sum = [v.x * 0.5 + 0.5, v.y * 0.5 + 0.5, v.z * 0.5 + 0.5, 1.0];
                }
                next[y * half + x] = sum;
            }
        }
        level = next;
        s = half;
    }
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let mut img = Image::new_uninit(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = levels;
    img.data = Some(data);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    img
}

// ---------------------------------------------------------------- meshes

/// A box of full size `size` whose texture repeats every `tile` metres on every face
/// (so a long wall shows more bricks, not stretched ones). Has tangents for normal maps.
pub fn tiled_box(size: Vec3, tile: f32) -> Mesh {
    let h = size / 2.0;
    let mut pos = Vec::new();
    let mut nor = Vec::new();
    let mut uv = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    // (normal, u axis, v axis) per face; v runs downward so textures stand upright.
    let faces = [
        (Vec3::X, Vec3::NEG_Z, Vec3::NEG_Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::NEG_Y),
        (Vec3::Z, Vec3::X, Vec3::NEG_Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::NEG_Y),
        (Vec3::Y, Vec3::X, Vec3::Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::NEG_Z),
    ];
    for (n, ua, va) in faces {
        let base = pos.len() as u32;
        let (du, dv) = ((ua * h).abs().max_element(), (va * h).abs().max_element());
        let centre = n * (n * h).abs().max_element();
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = centre + ua * du * su + va * dv * sv;
            pos.push(p.to_array());
            nor.push(n.to_array());
            // World-scaled UVs; offset by the face position so neighbours line up.
            let u = (p.dot(ua) + 1000.0) / tile;
            let v = (p.dot(va) + 1000.0) / tile;
            uv.push([u, v]);
        }
        idx.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx));
    mesh.clone().with_generated_tangents().unwrap_or(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_tiles_seamlessly() {
        for i in 0..20 {
            let v = i as f32 / 20.0;
            assert!((noise(0.0, v, 8, 1) - noise(1.0, v, 8, 1)).abs() < 1e-5);
            assert!((fbm(v, 0.0, 4, 2) - fbm(v, 1.0, 4, 2)).abs() < 1e-5);
        }
    }

    #[test]
    fn baked_images_have_full_mip_chains() {
        let (c, n) = bake(16, 1.0, &concrete);
        assert_eq!(c.texture_descriptor.mip_level_count, 5, "16, 8, 4, 2, 1");
        let bytes: usize = [16, 8, 4, 2, 1].iter().map(|s| s * s * 4).sum();
        assert_eq!(c.data.as_ref().unwrap().len(), bytes);
        assert_eq!(n.data.as_ref().unwrap().len(), bytes);
        assert_eq!(n.texture_descriptor.format, TextureFormat::Rgba8Unorm);
    }

    #[test]
    fn flat_height_gives_straight_up_normals() {
        let (_, n) = bake(8, 2.0, &|_, _| ([1.0; 3], 0.5));
        let d = n.data.unwrap();
        assert_eq!(&d[0..4], &[128, 128, 255, 255]);
    }

    #[test]
    fn tiled_box_uvs_follow_world_size() {
        let m = tiled_box(Vec3::new(8.0, 2.0, 1.0), 2.0);
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) = m.attribute(Mesh::ATTRIBUTE_UV_0) else {
            panic!("uvs")
        };
        // The +Z face (third) spans 8 m along X: 4 tiles.
        let us: Vec<f32> = uv[8..12].iter().map(|t| t[0]).collect();
        let span = us.iter().cloned().fold(f32::MIN, f32::max) - us.iter().cloned().fold(f32::MAX, f32::min);
        assert!((span - 4.0).abs() < 1e-4, "{span}");
        assert!(m.attribute(Mesh::ATTRIBUTE_TANGENT).is_some());
    }
}
