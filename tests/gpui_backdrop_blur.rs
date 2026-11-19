// Checks the vendored gpui patch's backdrop blur bookkeeping from outside the crate: gpui-pre's own lib
// tests don't build standalone. Covers how an element's transformation reaches its blur, and when the
// renderers' blur level textures shrink or go. Drop with the vendored gpui once upstream ships the feature.
use gpui_kit::{
    BackdropBlurTextureBudget, BackdropFilter, Bounds, Corners, DevicePixels, Pixels, Point, ScaledPixels,
    Size, TransformationMatrix, px, radians,
};

fn device_size(width: i32, height: i32) -> Size<DevicePixels> {
    Size::new(DevicePixels(width), DevicePixels(height))
}

fn filter() -> BackdropFilter {
    BackdropFilter { blur_radius: px(10.), saturation: 1.5, fade: [px(4.), px(2.), px(6.), px(8.)], clear_top: px(12.) }
}

fn element() -> (Bounds<Pixels>, Corners<Pixels>) {
    (Bounds::new(Point::new(px(100.), px(50.)), Size::new(px(200.), px(80.))), Corners::all(px(10.)))
}

#[test]
fn an_untransformed_blur_is_unchanged() {
    let (bounds, corner_radii) = element();
    let (mapped_bounds, mapped_radii, mapped) =
        filter().transformed(bounds, corner_radii, TransformationMatrix::unit(), 2.).unwrap();
    assert_eq!(mapped_bounds, bounds);
    assert_eq!(mapped_radii, corner_radii);
    assert_eq!(mapped, filter());
}

#[test]
fn a_scaled_and_translated_blur_lands_on_the_transformed_element() {
    let (bounds, corner_radii) = element();
    let scale_factor = 2.;
    // Scale by half about the element's center, then move it right 40 logical pixels, all in
    // scaled (device) pixels, like a popover's scale-in animation.
    let center = Point::new(ScaledPixels(400.), ScaledPixels(180.));
    let transformation = TransformationMatrix::unit()
        .translate(Point::new(ScaledPixels(80.), ScaledPixels(0.)))
        .translate(center)
        .scale(Size::new(0.5, 0.5))
        .translate(Point::new(ScaledPixels(-400.), ScaledPixels(-180.)));

    let (mapped_bounds, mapped_radii, mapped) =
        filter().transformed(bounds, corner_radii, transformation, scale_factor).unwrap();
    assert_eq!(mapped_bounds, Bounds::new(Point::new(px(190.), px(70.)), Size::new(px(100.), px(40.))));
    assert_eq!(mapped_radii, Corners::all(px(5.)));
    assert_eq!(mapped.blur_radius, px(5.));
    assert_eq!(mapped.fade, [px(2.), px(1.), px(3.), px(4.)]);
    assert_eq!(mapped.clear_top, px(6.));
    assert_eq!(mapped.saturation, 1.5);
}

#[test]
fn a_rotated_blur_is_skipped() {
    let (bounds, corner_radii) = element();
    let rotated = TransformationMatrix::unit().rotate(radians(0.3));
    assert!(filter().transformed(bounds, corner_radii, rotated, 1.).is_none());
    assert_eq!(rotated.axis_aligned_scale(), None);
    assert_eq!(
        TransformationMatrix::unit().scale(Size::new(-2., 3.)).axis_aligned_scale(),
        Some(Size::new(2., 3.))
    );
}

#[test]
fn blur_textures_grow_rounded_and_stay_while_used() {
    let mut budget = BackdropBlurTextureBudget::default();
    assert_eq!(budget.allocation_size(0, device_size(100, 30), None), Some(device_size(128, 64)));
    assert_eq!(budget.allocation_size(0, device_size(120, 60), Some(device_size(128, 64))), None);
    // Growing one side keeps the other at least as large as before.
    assert_eq!(budget.allocation_size(0, device_size(130, 10), Some(device_size(128, 64))), Some(device_size(192, 64)));
    for _ in 0..1000 {
        budget.allocation_size(0, device_size(130, 60), Some(device_size(192, 64)));
        assert_eq!(budget.end_frame(&[device_size(192, 64)]), 1);
    }
}

#[test]
fn blur_textures_go_after_frames_without_blurs() {
    let mut budget = BackdropBlurTextureBudget::default();
    let allocated = [device_size(3840, 2176), device_size(1920, 1088)];
    budget.allocation_size(0, allocated[0], Some(allocated[0]));
    budget.allocation_size(1, allocated[1], Some(allocated[1]));
    assert_eq!(budget.end_frame(&allocated), 2);

    let frames_kept = (0..1000).take_while(|_| budget.end_frame(&allocated) == 2).count();
    assert!(frames_kept > 0 && frames_kept < 100, "freed after {frames_kept} frames without blurs");

    // A blur in between restarts the count.
    let mut budget = BackdropBlurTextureBudget::default();
    for frame in 0..1000 {
        if frame % 10 == 0 {
            budget.allocation_size(0, allocated[0], Some(allocated[0]));
        }
        assert_eq!(budget.end_frame(&allocated[..1]), 1);
    }
}

#[test]
fn blur_textures_shrink_when_blurs_need_far_less() {
    let mut budget = BackdropBlurTextureBudget::default();
    let allocated = [device_size(3840, 2176), device_size(1920, 1088), device_size(960, 544)];
    // Blurs that needed a full window before now cover a small popover, without level 2. Level 0 is far
    // larger than needed, so every level goes, to come back at the size the popover needs.
    let kept = (0..1000)
        .map(|_| {
            budget.allocation_size(0, device_size(400, 300), Some(allocated[0]));
            budget.allocation_size(1, device_size(200, 150), Some(allocated[1]));
            budget.end_frame(&allocated)
        })
        .find(|&kept| kept != 3);
    assert_eq!(kept, Some(0));

    // Levels a little larger than needed stay; an unused last level goes.
    let mut budget = BackdropBlurTextureBudget::default();
    let snug = [device_size(448, 320), device_size(256, 192), device_size(128, 128)];
    let kept = (0..1000)
        .map(|_| {
            budget.allocation_size(0, device_size(400, 300), Some(snug[0]));
            budget.allocation_size(1, device_size(200, 150), Some(snug[1]));
            budget.end_frame(&snug)
        })
        .find(|&kept| kept != 3);
    assert_eq!(kept, Some(2));
}
