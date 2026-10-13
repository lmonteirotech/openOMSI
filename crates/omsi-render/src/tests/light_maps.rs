use super::*;

/// The middle pixel of a display - a coloured texture with a white `[matl_lightmap]`, its
/// light on - seen face on in the shade of a day (ambient light only), and by night, in the
/// classic, the Vanilla+ and the Enhanced picture.
fn display_pixel(night: f32, classic: bool, enhanced: bool) -> [u8; 3] {
    let adapter = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let mut renderer = pollster::block_on(Renderer::new_with(
        &adapter,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions { msaa: 1, ssao: false, shadow_size: 1024, fxaa: false, render_scale: 1.0, ..Default::default() },
    ))
    .expect("test renderer");
    let mut scene = renderer.new_scene();
    let one = |renderer: &mut Renderer, scene: &mut Scene, rgba: [u8; 4]| renderer.add_texture(scene, &omsi_texture::Image { width: 1, height: 1, rgba: rgba.to_vec(), has_alpha: false }, false);
    let face = one(&mut renderer, &mut scene, [60, 170, 220, 255]);
    let white = one(&mut renderer, &mut scene, [255, 255, 255, 255]);
    let mat = renderer.add_material_lit(&mut scene, Some(face), AlphaMode::Opaque, [1.0; 4], false, None, None, Some(white));
    let half = 2.0;
    let mesh = renderer.add_mesh(
        &mut scene,
        &MeshData {
            positions: vec![Vec3::new(-half, 4.0, -half), Vec3::new(half, 4.0, -half), Vec3::new(half, 4.0, half), Vec3::new(-half, 4.0, half)],
            normals: vec![-Vec3::Y; 4],
            uvs: vec![glam::Vec2::ZERO; 4],
            ranges: vec![(0, 6, 0)],
            indices: vec![0, 1, 2, 0, 2, 3],
            one_sided: false,
        },
    );
    let i = renderer.add_instance(&mut scene, mesh, DVec3::ZERO, Mat4::IDENTITY, vec![mat]);
    renderer.set_slot_light(&mut scene, i, &[1.0]);
    let camera = Camera { position: DVec3::ZERO, yaw: 0.0, pitch: 0.0, roll: 0.0, fov_deg: 60.0, near: 0.1, far: 100.0 };
    let day = 1.0 - night;
    let lighting = Lighting {
        classic,
        enhanced,
        shadows: false,
        fog_density: 0.0,
        night,
        // the sun behind the display: its face in the shade, lit by the sky alone
        sun_dir: Vec3::new(0.0, 1.0, 0.5).normalize(),
        sun_intensity: day,
        ambient: Vec3::splat(0.05 + 0.4 * day),
        ..Default::default()
    };
    let rgba = renderer.render_to_image(&mut scene, 64, 64, &camera, &lighting).unwrap();
    let c = &rgba[(32 * 64 + 32) * 4..][..3];
    [c[0], c[1], c[2]]
}

/// Omsi.exe lays a `[matl_lightmap]` onto the light with ADDSMOOTH (light + map x (1 -
/// light)): a white map lifts the display to its texture's own colour by day as by night.
/// Enhanced showed it by day at an eighth of that lift, a display in the cab's shade far
/// darker than in Vanilla+ (#2051).
#[test]
#[ignore = "requires a graphics adapter; run with --ignored on a GPU host"]
fn a_white_light_map_shows_the_display_by_day_as_by_night() {
    for night in [0.0, 1.0] {
        let plus = display_pixel(night, false, false);
        let enh = display_pixel(night, false, true);
        let sum = |c: [u8; 3]| c.iter().map(|&v| v as f32).sum::<f32>();
        assert!(sum(enh) > 0.75 * sum(plus), "night {night}: Enhanced {enh:?} against Vanilla+ {plus:?}");
        // (its colour, not white)
        assert!(enh[2] > enh[0] + 40, "night {night}: {enh:?}");
    }
}
