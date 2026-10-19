//! Native Slint measurement fixture. Uses the application's generated Dashboard unchanged.
//! This is an offscreen render, not an interactive Windows/DWM screenshot.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};
use std::{path::Path, rc::Rc};

mod ui {
    include!(env!("MOGU_UI_MODULE"));
}

slint::slint! {
    export component Calibration inherits Window {
        width: 104px; height: 104px; background: #fffdf8;
        Rectangle { x: 20px; y: 10px; width: 0.75px; height: 84px; background: #4b5563; }
        Rectangle { x: 40px; y: 10px; width: 1px; height: 84px; background: #4b5563; }
        Rectangle { x: 60px; y: 10px; width: 1.5px; height: 84px; background: #4b5563; }
        Rectangle { x: 80px; y: 10px; width: 2px; height: 84px; background: #4b5563; }
    }
}

struct RenderPlatform(Rc<MinimalSoftwareWindow>);
impl Platform for RenderPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn write_ppm(path: &Path, pixels: &[Rgb8Pixel], stride: usize, rect: [usize; 4]) {
    let [x, y, width, height] = rect;
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    for row in y..y + height {
        bytes.extend(pixels[row * stride + x..row * stride + x + width].iter().flat_map(|p| [p.r, p.g, p.b]));
    }
    std::fs::write(path, bytes).unwrap();
}

fn main() {
    let output = std::env::args_os().nth(1).expect("output directory");
    let output = Path::new(&output);
    std::fs::create_dir_all(output).unwrap();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(RenderPlatform(window.clone()))).unwrap();
    let dashboard = ui::Dashboard::new().unwrap();
    dashboard.global::<ui::Store>().set_demo(true);
    dashboard.global::<ui::Store>().set_version("0.1.0".into());
    dashboard.show().unwrap();
    for (width, height) in [(900, 560), (1080, 680)] {
        for percent in [100, 125, 150, 200, 400] {
            // The scale-factor event maps logical UI coordinates to physical pixels.
            let scale = percent as f32 / 100.0;
            window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
            let physical_width = (width as f32 * scale).round() as usize;
            let physical_height = (height as f32 * scale).round() as usize;
            window.set_size(PhysicalSize::new(physical_width as u32, physical_height as u32));
            assert_eq!(window.scale_factor(), scale);
            for page in 0..8 {
                dashboard.global::<ui::Store>().set_page(page);
                slint::platform::update_timers_and_animations();
                window.request_redraw();
                let mut pixels = vec![Rgb8Pixel::default(); physical_width * physical_height];
                assert!(window.draw_if_needed(|renderer| {
                    renderer.render(&mut pixels, physical_width);
                }));
                // The 148-wide sidebar image is centered at x=30, with a 104px square
                // viewport ending 96px above the window's bottom (ui/app.slint).
                let rect = [
                    (30.0 * scale).round() as usize,
                    ((height - 200) as f32 * scale).round() as usize,
                    (104.0 * scale).round() as usize,
                    (104.0 * scale).round() as usize,
                ];
                write_ppm(
                    &output.join(format!("{width}x{height}-{percent}-{page}.ppm")),
                    &pixels,
                    physical_width,
                    rect,
                );
                if width == 900 && percent == 100 && [0, 1, 2, 7].contains(&page) {
                    write_ppm(
                        &output.join(format!("app-{page}.ppm")),
                        &pixels,
                        physical_width,
                        [0, 0, physical_width, physical_height],
                    );
                }
            }
        }
    }
    dashboard.hide().unwrap();
    drop(dashboard);
    let calibration = Calibration::new().unwrap();
    calibration.show().unwrap();
    for percent in [100, 125, 150, 200, 400] {
        let scale = percent as f32 / 100.0;
        window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
        let side = (104.0 * scale).round() as usize;
        window.set_size(PhysicalSize::new(side as u32, side as u32));
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        let mut pixels = vec![Rgb8Pixel::default(); side * side];
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, side);
        }));
        write_ppm(&output.join(format!("calibration-{percent}.ppm")), &pixels, side, [0, 0, side, side]);
    }
    calibration.hide().unwrap();
    println!("80 native mascot renders: 8 views, 2 window sizes, 5 DPI scales; no user data opened.");
    println!("5 calibration renders with known 0.75/1/1.5/2px line widths.");
}
