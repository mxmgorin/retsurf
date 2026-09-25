use crate::config::token_enum::token_enum;

token_enum! {
    /// Which pad the device has: what each face button prints, and where.
    pub enum PadLayout {
        default Nintendo;
        /// A east, B south, X north, Y west.
        Nintendo => "nintendo", "Nintendo",
        /// A south, B east, X west, Y north.
        Xbox => "xbox", "Xbox",
        /// Cross south, circle east, square west, triangle north.
        PlayStation => "playstation", "PlayStation",
    }
}

/// A face button by where it sits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    North,
    West,
    East,
    South,
}

/// Where each face button sits, by the Xbox letter it reads as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FacePlaces {
    pub a: Face,
    pub b: Face,
    pub x: Face,
    pub y: Face,
}

/// What each face button prints, named by the Xbox letter it reads as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaceLabels {
    pub a: &'static str,
    pub b: &'static str,
    pub x: &'static str,
    pub y: &'static str,
}

impl PadLayout {
    /// Where this pad prints each letter.
    pub fn places(self) -> FacePlaces {
        match self {
            PadLayout::Nintendo => FacePlaces {
                a: Face::East,
                b: Face::South,
                x: Face::North,
                y: Face::West,
            },
            PadLayout::Xbox | PadLayout::PlayStation => FacePlaces {
                a: Face::South,
                b: Face::East,
                x: Face::West,
                y: Face::North,
            },
        }
    }

    pub fn labels(self) -> FaceLabels {
        match self {
            PadLayout::Nintendo | PadLayout::Xbox => FaceLabels {
                a: "A",
                b: "B",
                x: "X",
                y: "Y",
            },
            PadLayout::PlayStation => FaceLabels {
                a: egui_phosphor::bold::X,
                b: egui_phosphor::bold::CIRCLE,
                x: egui_phosphor::bold::SQUARE,
                y: egui_phosphor::bold::TRIANGLE,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nintendo_prints_a_on_the_east() {
        assert_eq!(PadLayout::Nintendo.places().a, Face::East);
        assert_eq!(PadLayout::Xbox.places().a, Face::South);
    }
}
