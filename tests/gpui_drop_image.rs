// Why images.rs defers `drop_image`: gpui takes a window out of `App` while it renders or runs its
// callbacks, so a drop made from inside never reaches that window's atlas and its GPU pages stay.
use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{Context, RenderImage, TestAppContext, VisualTestContext, Window, div, img, px};

struct Picture(Arc<RenderImage>);

impl Render for Picture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(img(self.0.clone()).size(px(4.)))
    }
}

#[gpui_kit::test]
fn a_drop_inside_the_window_only_lands_deferred(cx: &mut TestAppContext) {
    let picture = Arc::new(RenderImage::new([image::Frame::new(image::RgbaImage::new(4, 4))]));
    let (_, cx) = cx.add_window_view(|_, _| Picture(picture.clone()));
    cx.run_until_parked();
    assert!(in_atlas(cx, &picture), "drawn into the window's atlas");

    let image = picture.clone();
    cx.update(|_, cx| cx.drop_image(image, None));
    assert!(in_atlas(cx, &picture), "dropped from inside the window: missed");

    let image = picture.clone();
    cx.update(|_, cx| cx.defer(move |cx| cx.drop_image(image, None)));
    assert!(!in_atlas(cx, &picture), "deferred: freed");
}

fn in_atlas(cx: &mut VisualTestContext, picture: &RenderImage) -> bool {
    cx.update(|window, _| window.has_image_atlas_entry(picture))
}
