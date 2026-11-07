//! Posição do painel perto do ícone da bandeja. Lógica pura: só coordenadas em pixels físicos
//! (origem no canto superior esquerdo da tela). Quem chama converte de/para o Tauri.

/// Retângulo em pixels físicos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Tamanho em pixels físicos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

/// Ponto em pixels físicos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Distância entre o painel e a borda da área de trabalho (a barra de tarefas), em pixels
/// físicos: 1 px, o mínimo para o painel não encostar na barra.
const MARGIN: i32 = 1;

impl Rect {
    fn right(&self) -> i32 {
        self.x + self.width
    }

    fn bottom(&self) -> i32 {
        self.y + self.height
    }

    fn centre(&self) -> Point {
        Point {
            x: self.x + self.width / 2,
            y: self.y + self.height / 2,
        }
    }
}

/// Lado da tela onde fica a barra de tarefas. O painel abre do lado oposto ao ícone, afastado dele.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

/// De que lado da área de trabalho fica o ícone. O ícone costuma ficar fora dela (na barra de
/// tarefas), então o lado é o de onde ele está fora. Dentro da área (barra escondida, ícone
/// num transbordo) vale o lado mais perto.
fn edge_of(tray: Rect, work: Rect) -> Edge {
    let c = tray.centre();
    if c.y >= work.bottom() {
        return Edge::Bottom;
    }
    if c.y < work.y {
        return Edge::Top;
    }
    if c.x >= work.right() {
        return Edge::Right;
    }
    if c.x < work.x {
        return Edge::Left;
    }
    let gaps = [
        (work.bottom() - c.y, Edge::Bottom),
        (c.y - work.y, Edge::Top),
        (work.right() - c.x, Edge::Right),
        (c.x - work.x, Edge::Left),
    ];
    gaps.into_iter()
        .min_by_key(|(gap, _)| *gap)
        .map_or(Edge::Bottom, |(_, edge)| edge)
}

/// Canto superior esquerdo do painel para abrir perto do ícone `tray`, sempre dentro de
/// `work_area` (a área de trabalho do monitor, sem a barra de tarefas). Com a barra embaixo,
/// o painel abre acima dela; com a barra em cima, abaixo; nas laterais, ao lado.
pub fn place_near_tray(tray: Rect, work_area: Rect, window: Size) -> Point {
    let c = tray.centre();
    let (x, y) = match edge_of(tray, work_area) {
        Edge::Bottom => (
            c.x - window.width / 2,
            work_area.bottom() - window.height - MARGIN,
        ),
        Edge::Top => (c.x - window.width / 2, work_area.y + MARGIN),
        Edge::Right => (
            work_area.right() - window.width - MARGIN,
            c.y - window.height / 2,
        ),
        Edge::Left => (work_area.x + MARGIN, c.y - window.height / 2),
    };
    clamp_inside(Point { x, y }, work_area, window)
}

/// Canto inferior direito da área de trabalho: onde o painel abre quando não se conhece o
/// ícone (atalho antes de qualquer clique na bandeja).
pub fn corner_fallback(work_area: Rect, window: Size) -> Point {
    clamp_inside(
        Point {
            x: work_area.right() - window.width - MARGIN,
            y: work_area.bottom() - window.height - MARGIN,
        },
        work_area,
        window,
    )
}

/// Empurra o painel para dentro da área de trabalho. Usa `min`/`max` e não `clamp`: se o painel
/// for maior que a área, `clamp` entraria em pânico; aqui o canto de cima e da esquerda ganha.
fn clamp_inside(p: Point, work: Rect, window: Size) -> Point {
    let max_x = work.right() - window.width;
    let max_y = work.bottom() - window.height;
    Point {
        x: p.x.min(max_x).max(work.x),
        y: p.y.min(max_y).max(work.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: Size = Size {
        width: 420,
        height: 700,
    };

    /// Monitor de 1920x1080 com barra de tarefas de 40 px embaixo: área de trabalho 1920x1040.
    fn bottom_work_area() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        }
    }

    #[test]
    fn place_near_tray_bottom_taskbar_opens_above() {
        let tray = Rect {
            x: 1880,
            y: 1050,
            width: 24,
            height: 24,
        };

        let p = place_near_tray(tray, bottom_work_area(), PANEL);

        assert!(p.y + PANEL.height <= 1040, "painel acima da barra");
        assert!(p.y < tray.y, "painel fica acima do ícone");
        assert!(p.x + PANEL.width <= 1920, "painel dentro da tela");
    }

    #[test]
    fn place_near_tray_bottom_taskbar_is_one_pixel_away_and_centred_on_the_icon() {
        // O painel encosta a 1 px da barra e fica centrado no ícone.
        let tray = Rect {
            x: 1000,
            y: 1050,
            width: 24,
            height: 24,
        };

        let p = place_near_tray(tray, bottom_work_area(), PANEL);

        assert_eq!(p.y + PANEL.height, 1040 - 1, "1 px acima da barra");
        assert_eq!(p.x + PANEL.width / 2, tray.x + tray.width / 2, "centrado no ícone");
    }

    #[test]
    fn place_near_tray_top_taskbar_opens_below() {
        // Barra em cima: a área de trabalho começa em y = 40.
        let work = Rect {
            x: 0,
            y: 40,
            width: 1920,
            height: 1040,
        };
        let tray = Rect {
            x: 1880,
            y: 4,
            width: 24,
            height: 24,
        };

        let p = place_near_tray(tray, work, PANEL);

        assert!(p.y >= tray.y + tray.height, "painel abre abaixo do ícone");
        assert!(p.y >= work.y);
        assert!(p.y + PANEL.height <= work.bottom());
    }

    #[test]
    fn place_near_tray_left_and_right_taskbars_open_beside() {
        // Barra à esquerda: área começa em x = 40.
        let left_work = Rect {
            x: 40,
            y: 0,
            width: 1880,
            height: 1080,
        };
        let left_tray = Rect {
            x: 4,
            y: 500,
            width: 24,
            height: 24,
        };
        let p = place_near_tray(left_tray, left_work, PANEL);
        assert!(p.x >= left_work.x && p.x < left_tray.x + left_tray.width + PANEL.width);
        assert!(p.x >= left_tray.x + left_tray.width, "painel fica à direita da barra");

        // Barra à direita: área termina em x = 1880.
        let right_work = Rect {
            x: 0,
            y: 0,
            width: 1880,
            height: 1080,
        };
        let right_tray = Rect {
            x: 1900,
            y: 500,
            width: 24,
            height: 24,
        };
        let p = place_near_tray(right_tray, right_work, PANEL);
        assert!(p.x + PANEL.width <= right_work.right(), "painel à esquerda da barra");
    }

    #[test]
    fn place_near_tray_clamps_inside_work_area() {
        // Área menor que o painel: o canto de cima e da esquerda ganha, sem pânico.
        let small = Rect {
            x: 0,
            y: 0,
            width: 300,
            height: 300,
        };
        let tray = Rect {
            x: 280,
            y: 280,
            width: 16,
            height: 16,
        };

        assert_eq!(place_near_tray(tray, small, PANEL), Point { x: 0, y: 0 });

        // Ícone colado na borda esquerda de uma área normal: o painel não sai pela esquerda.
        let p = place_near_tray(
            Rect {
                x: 2,
                y: 1050,
                width: 24,
                height: 24,
            },
            bottom_work_area(),
            PANEL,
        );
        assert_eq!(p.x, 0);
    }

    #[test]
    fn corner_fallback_is_bottom_right_of_work_area() {
        assert_eq!(
            corner_fallback(bottom_work_area(), PANEL),
            Point { x: 1499, y: 339 }
        );
    }

    #[test]
    fn scaled_window_size_still_fits_inside() {
        // 150%: a janela tem 630x1050 pixels físicos; monitor 1920x1080 sem barra embaixo.
        let scaled = Size {
            width: 630,
            height: 1050,
        };
        let work = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let tray = Rect {
            x: 1860,
            y: 1080,
            width: 36,
            height: 36,
        };

        let p = place_near_tray(tray, work, scaled);

        assert!(p.y + scaled.height <= 1080);
        assert!(p.x + scaled.width <= 1920);
        assert!(p.x >= 0 && p.y >= 0);
    }
}
