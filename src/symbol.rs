macro_rules! sym {
    ($id:ident, $val:literal) => {
        pub const $id: &'static str = $val;
    };
}

sym!(PAUSE, "󰏤");
sym!(PLAY, "󰐊");
