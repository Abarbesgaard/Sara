use super::*;

#[test]
fn xterm256_picks_the_nearest_cube_or_grey_entry() {
    assert_eq!(xterm256(0x000000), 16);
    assert_eq!(xterm256(0xffffff), 231);
    assert_eq!(xterm256(0x808080), 244);
    assert_eq!(xterm256(0xff0000), 196);
    assert_eq!(xterm256(0x00ffff), 51);
}
