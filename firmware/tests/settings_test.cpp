#include "../main/settings.h"

#include <cassert>

int main() {
    using namespace settings;
    Button boot(true);
    assert(boot.update(true, 900, true) == Press::none);
    boot.update(false, 910, true);
    assert(boot.update(false, 940, true) == Press::none);
    boot.update(true, 1000, true);
    boot.update(false, 1010, true);
    assert(boot.update(false, 1040, true) == Press::none);
    boot.update(true, 1100, true);
    assert(boot.update(true, 1130, true) == Press::none);
    assert(boot.update(true, 1899, true) == Press::none);
    assert(boot.update(true, 1900, true) == Press::hold);
    assert(boot.update(true, 2500, true) == Press::none);
    boot.update(false, 2510, true);
    assert(boot.update(false, 2540, true) == Press::none);
    boot.update(true, 2600, true);
    boot.update(true, 2630, true);
    boot.update(false, 2700, true);
    assert(boot.update(false, 2730, true) == Press::tap);

    Menu menu;
    assert(menu.update(Press::tap, Press::none, 70) == Action::next_page);
    menu.update(Press::none, Press::hold, 70);
    assert(menu.view == View::menu && menu.selected == 0);
    menu.update(Press::none, Press::tap, 70);
    assert(menu.view == View::brightness && menu.brightness == 70);
    menu.update(Press::tap, Press::none, 70);
    assert(menu.brightness == 60);
    assert(menu.update(Press::none, Press::hold, 70) == Action::save);
    menu.saved(false);
    assert(menu.view == View::brightness && menu.save_failed);
    menu.saved(true);
    assert(menu.view == View::menu && !menu.save_failed);
    menu.update(Press::tap, Press::none, 60);
    assert(menu.selected == 1);
    assert(menu.update(Press::none, Press::tap, 60) == Action::toggle_rotation);
    assert(menu.view == View::menu && menu.selected == 1);
    menu.update(Press::tap, Press::none, 60);
    menu.update(Press::none, Press::tap, 60);
    assert(menu.view == View::about);
    menu.update(Press::none, Press::tap, 60);
    assert(menu.view == View::menu);
    menu.update(Press::tap, Press::none, 60);
    menu.update(Press::none, Press::tap, 60);
    assert(menu.view == View::dashboard);

    menu.update(Press::none, Press::hold, 0);
    menu.update(Press::none, Press::tap, 0);
    assert(menu.output_brightness(0) == 10);
    assert(menu.update(Press::none, Press::hold, 0) == Action::none);
    menu.update(Press::none, Press::hold, 0);
    assert(menu.output_brightness(0) == 0);
}
