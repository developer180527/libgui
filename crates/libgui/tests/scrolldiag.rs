use libgui::*;

fn ui() -> Ui { Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf")).unwrap() }

/// Plain scroll area: wheel once, then run with no input and watch the ease.
#[test]
fn trace_plain() {
    let mut ui = ui();
    let mut frame = |ui: &mut Ui| -> (f32, Option<f32>) {
        ui.begin_frame(FrameInfo::default());
        let mut y = 0.0;
        ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(300.0)), |ui| {
            for i in 0..60 {
                let id = ui.make_id(("r", i));
                ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(28.0)), Vec2::ZERO, true, |_, _| {});
                if i == 0 { y = ui.rect_of(id).unwrap_or_default().y; }
            }
        });
        let out = ui.end_frame();
        (y, out.platform.repaint_after)
    };
    frame(&mut ui); frame(&mut ui);
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(50.0, 100.0) });
    ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -120.0), unit: WheelUnit::Pixel });
    println!("--- plain scroll area: one wheel notch of -120 ---");
    for i in 0..14 {
        let (y, rep) = frame(&mut ui);
        println!("frame {i:2}: row0.y = {y:8.2}   repaint_after = {rep:?}");
    }
}

/// Virtual list: same thing, since the demo's outliner uses one.
#[test]
fn trace_virtual() {
    let mut ui = ui();
    let names: Vec<String> = (0..2000).map(|i| format!("Object {i}")).collect();
    let mut frame = |ui: &mut Ui| -> (f32, Option<f32>, usize) {
        ui.begin_frame(FrameInfo::default());
        let mut y = 0.0;
        let mut n = 0;
        let opts = ListOptions { height: Size::Fixed(300.0), ..ListOptions::new(24.0) };
        let built = ui.virtual_list_with("rows", 2000, opts, |ui, i| {
            let _ = ui.selectable_keyed(i, &names[i], false);
            if i == 0 { y = ui.rect_of(ui.make_id("probe")).unwrap_or_default().y; }
        });
        n = built.len();
        let out = ui.end_frame();
        (y, out.platform.repaint_after, n)
    };
    frame(&mut ui); frame(&mut ui);
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(50.0, 100.0) });
    ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -120.0), unit: WheelUnit::Pixel });
    println!("--- virtual list: one wheel notch of -120 ---");
    for i in 0..14 {
        let (_, rep, n) = frame(&mut ui);
        println!("frame {i:2}: built = {n:3}   repaint_after = {rep:?}");
    }
}
