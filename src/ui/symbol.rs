macro_rules! sym {
    ($id:ident, $val:literal) => {
        #[allow(unused)]
        pub const $id: &'static str = $val;
    };
}

sym!(PAUSE, "󰏤");
sym!(PLAY, "󰐊");
sym!(SHUFFLE, "󰒟");
sym!(REPEAT, "󰑖");
sym!(REPEAT_ONE, "󰑘");
sym!(REPEAT_OFF, "󰑗");
sym!(MUSIC_NOTES, "󰝚");
sym!(DIAL_INDICATOR_LOW, "󰾆");
sym!(DIAL_INDICATOR_HIGH, "󰓅");
sym!(OCTAGON, "󰏃");
