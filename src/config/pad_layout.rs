use crate::config::token_enum::token_enum;

token_enum! {
    /// Which pad the device has: where each letter is printed, and in which
    /// colour. Hints and the wheel follow it; which press is A is the swap's.
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
