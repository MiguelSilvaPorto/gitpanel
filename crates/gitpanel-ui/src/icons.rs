use egui::{Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, vec2};

#[derive(Clone, Copy, Debug)]
pub enum Icon {
    Repo,
    Lock,
    Globe,
    Star,
    Branch,
    Commit,
    Search,
    Sun,
    Moon,
    Refresh,
    ChevronDown,
    Settings,
    Plus,
    Warning,
    Check,
    External,
    Chart,
    Diff,
    Cloud,
    Local,
    Folder,
    File,
}

pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
    let size = Vec2::splat(28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let color = if response.hovered() {
        ui.visuals().widgets.hovered.fg_stroke.color
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    draw(ui, icon, rect, color);
    response.on_hover_text(tooltip)
}

pub fn icon(ui: &mut Ui, icon: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    draw(ui, icon, rect, color);
}

pub fn draw(ui: &Ui, icon: Icon, rect: Rect, color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.4, color);
    let r = rect.shrink(rect.width() * 0.22);
    let center = r.center();
    match icon {
        Icon::Repo => {
            painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    r.left_top() + vec2(5.0, 0.0),
                    r.left_bottom() + vec2(5.0, 0.0),
                ],
                stroke,
            );
            for offset in [0.32, 0.52, 0.72] {
                let y = r.top() + r.height() * offset;
                painter.line_segment(
                    [Pos2::new(r.left() + 8.0, y), Pos2::new(r.right() - 4.0, y)],
                    Stroke::new(1.0, color),
                );
            }
        }
        Icon::Lock => {
            let body = Rect::from_min_max(
                Pos2::new(r.left(), center.y - 2.0),
                Pos2::new(r.right(), r.bottom()),
            );
            painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);
            painter.circle_stroke(
                Pos2::new(center.x, center.y - 6.0),
                r.width() * 0.28,
                stroke,
            );
        }
        Icon::Globe => {
            painter.circle_stroke(center, r.width() * 0.44, stroke);
            painter.line_segment(
                [
                    Pos2::new(r.left() + 2.0, center.y),
                    Pos2::new(r.right() - 2.0, center.y),
                ],
                stroke,
            );
            painter.add(Shape::line(
                vec![
                    Pos2::new(center.x, r.top()),
                    Pos2::new(center.x - 5.0, center.y),
                    Pos2::new(center.x, r.bottom()),
                    Pos2::new(center.x + 5.0, center.y),
                    Pos2::new(center.x, r.top()),
                ],
                Stroke::new(1.0, color),
            ));
        }
        Icon::Star => {
            let points = star_points(center, r.width() * 0.5, r.width() * 0.22, 5);
            painter.add(Shape::closed_line(points, stroke));
        }
        Icon::Branch => {
            painter.circle_stroke(Pos2::new(center.x - 5.0, r.top() + 2.0), 3.0, stroke);
            painter.circle_stroke(Pos2::new(center.x - 5.0, r.bottom() - 2.0), 3.0, stroke);
            painter.circle_stroke(Pos2::new(center.x + 5.0, center.y), 3.0, stroke);
            painter.line_segment(
                [
                    Pos2::new(center.x - 5.0, r.top() + 5.0),
                    Pos2::new(center.x - 5.0, r.bottom() - 5.0),
                ],
                stroke,
            );
            painter.add(Shape::line(
                vec![
                    Pos2::new(center.x - 2.0, r.top() + 6.0),
                    Pos2::new(center.x + 2.0, center.y - 2.0),
                    Pos2::new(center.x + 5.0, center.y - 3.0),
                ],
                stroke,
            ));
        }
        Icon::Commit => {
            painter.circle_stroke(center, 3.5, stroke);
            painter.line_segment(
                [
                    Pos2::new(r.left(), center.y),
                    Pos2::new(center.x - 4.0, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 4.0, center.y),
                    Pos2::new(r.right(), center.y),
                ],
                stroke,
            );
        }
        Icon::Search => {
            painter.circle_stroke(
                Pos2::new(center.x - 2.0, center.y - 2.0),
                r.width() * 0.3,
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 2.0, center.y + 2.0),
                    Pos2::new(r.right(), r.bottom()),
                ],
                stroke,
            );
        }
        Icon::Sun => {
            painter.circle_stroke(center, r.width() * 0.22, stroke);
            for index in 0..8 {
                let angle = index as f32 * std::f32::consts::TAU / 8.0;
                let inner = center + Vec2::angled(angle) * (r.width() * 0.32);
                let outer = center + Vec2::angled(angle) * (r.width() * 0.46);
                painter.line_segment([inner, outer], Stroke::new(1.0, color));
            }
        }
        Icon::Moon => {
            painter.circle_stroke(center, r.width() * 0.34, stroke);
            painter.add(Shape::line(
                vec![
                    Pos2::new(center.x + 4.0, r.top() + 1.0),
                    Pos2::new(center.x - 1.0, center.y),
                    Pos2::new(center.x + 4.0, r.bottom() - 1.0),
                ],
                Stroke::new(1.0, color),
            ));
        }
        Icon::Refresh => {
            painter.circle_stroke(center, r.width() * 0.36, stroke);
            painter.add(Shape::line(
                vec![
                    Pos2::new(r.right() - 1.0, center.y - 5.0),
                    Pos2::new(r.right() - 1.0, center.y),
                    Pos2::new(r.right() - 6.0, center.y),
                ],
                stroke,
            ));
        }
        Icon::ChevronDown => {
            painter.add(Shape::line(
                vec![
                    Pos2::new(center.x - 4.0, center.y - 2.0),
                    Pos2::new(center.x, center.y + 2.0),
                    Pos2::new(center.x + 4.0, center.y - 2.0),
                ],
                stroke,
            ));
        }
        Icon::Settings => {
            painter.circle_stroke(center, r.width() * 0.2, stroke);
            for index in 0..6 {
                let angle = index as f32 * std::f32::consts::TAU / 6.0;
                let inner = center + Vec2::angled(angle) * (r.width() * 0.3);
                let outer = center + Vec2::angled(angle) * (r.width() * 0.44);
                painter.line_segment([inner, outer], Stroke::new(2.0, color));
            }
        }
        Icon::Plus => {
            painter.line_segment(
                [
                    Pos2::new(center.x - 6.0, center.y),
                    Pos2::new(center.x + 6.0, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x, center.y - 6.0),
                    Pos2::new(center.x, center.y + 6.0),
                ],
                stroke,
            );
        }
        Icon::Warning => {
            painter.add(Shape::closed_line(
                vec![
                    Pos2::new(center.x, r.top()),
                    Pos2::new(r.right(), r.bottom()),
                    Pos2::new(r.left(), r.bottom()),
                ],
                stroke,
            ));
            painter.line_segment(
                [
                    Pos2::new(center.x, center.y - 4.0),
                    Pos2::new(center.x, center.y + 3.0),
                ],
                stroke,
            );
            painter.circle_filled(Pos2::new(center.x, center.y + 7.0), 1.0, color);
        }
        Icon::Check => {
            painter.add(Shape::line(
                vec![
                    Pos2::new(r.left() + 1.0, center.y),
                    Pos2::new(center.x - 2.0, r.bottom() - 2.0),
                    Pos2::new(r.right() - 1.0, r.top() + 2.0),
                ],
                Stroke::new(1.8, color),
            ));
        }
        Icon::External => {
            painter.rect_stroke(
                Rect::from_min_max(
                    Pos2::new(r.left(), r.top() + 5.0),
                    Pos2::new(r.right() - 5.0, r.bottom()),
                ),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [Pos2::new(center.x, center.y), Pos2::new(r.right(), r.top())],
                stroke,
            );
            painter.add(Shape::line(
                vec![
                    Pos2::new(center.x + 5.0, r.top()),
                    Pos2::new(r.right(), r.top()),
                    Pos2::new(r.right(), r.top() + 5.0),
                ],
                stroke,
            ));
        }
        Icon::Chart => {
            for (index, height) in [0.35, 0.65, 0.5, 0.8].into_iter().enumerate() {
                let x = r.left() + 2.0 + index as f32 * (r.width() - 4.0) / 4.0;
                let top = r.bottom() - r.height() * height;
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(x, top), Pos2::new(x + 3.0, r.bottom())),
                    1.0,
                    color,
                );
            }
        }
        Icon::Diff => {
            painter.line_segment(
                [
                    Pos2::new(center.x - 5.0, r.top()),
                    Pos2::new(center.x - 5.0, r.bottom()),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.0, r.top() + 4.0),
                    Pos2::new(r.right(), r.top() + 4.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.0, r.bottom() - 4.0),
                    Pos2::new(r.right(), r.bottom() - 4.0),
                ],
                stroke,
            );
        }
        Icon::Cloud => {
            painter.circle_stroke(
                Pos2::new(center.x - 5.0, center.y + 1.0),
                r.width() * 0.24,
                stroke,
            );
            painter.circle_stroke(
                Pos2::new(center.x + 2.0, center.y - 2.0),
                r.width() * 0.32,
                stroke,
            );
            painter.circle_stroke(
                Pos2::new(center.x + 7.0, center.y + 2.0),
                r.width() * 0.2,
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(r.left() + 1.0, center.y + 5.0),
                    Pos2::new(r.right() - 1.0, center.y + 5.0),
                ],
                stroke,
            );
        }
        Icon::Local => {
            painter.rect_stroke(r, 3.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(r.left() + 3.0, center.y - 4.0),
                    Pos2::new(r.right() - 3.0, center.y - 4.0),
                ],
                stroke,
            );
            painter.circle_filled(Pos2::new(r.right() - 4.0, center.y + 3.0), 1.5, color);
        }
        Icon::Folder => {
            painter.rect_stroke(
                Rect::from_min_max(
                    Pos2::new(r.left(), r.top() + 3.0),
                    Pos2::new(r.right(), r.bottom()),
                ),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.add(Shape::line(
                vec![
                    Pos2::new(r.left(), r.top() + 3.0),
                    Pos2::new(r.left(), r.top()),
                    Pos2::new(r.left() + 6.0, r.top()),
                    Pos2::new(r.left() + 8.0, r.top() + 3.0),
                ],
                stroke,
            ));
        }
        Icon::File => {
            painter.add(Shape::line(
                vec![
                    Pos2::new(r.left() + 2.0, r.top()),
                    Pos2::new(r.right() - 5.0, r.top()),
                    Pos2::new(r.right(), r.top() + 5.0),
                    Pos2::new(r.right(), r.bottom()),
                    Pos2::new(r.left() + 2.0, r.bottom()),
                    Pos2::new(r.left() + 2.0, r.top()),
                ],
                stroke,
            ));
            painter.line_segment(
                [
                    Pos2::new(r.right() - 5.0, r.top()),
                    Pos2::new(r.right() - 5.0, r.top() + 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(r.right() - 5.0, r.top() + 5.0),
                    Pos2::new(r.right(), r.top() + 5.0),
                ],
                stroke,
            );
        }
    }
}

fn star_points(center: Pos2, outer: f32, inner: f32, points: usize) -> Vec<Pos2> {
    let mut result = Vec::with_capacity(points * 2);
    for index in 0..points * 2 {
        let radius = if index % 2 == 0 { outer } else { inner };
        let angle =
            -std::f32::consts::FRAC_PI_2 + index as f32 * std::f32::consts::PI / points as f32;
        result.push(center + Vec2::angled(angle) * radius);
    }
    result
}
