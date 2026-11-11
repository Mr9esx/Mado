#pragma once

#include <algorithm>
#include <cstdint>

namespace settings {
enum class Press { none, tap, hold };
enum class View { dashboard, menu, brightness, about, bluetooth, wifi };
enum class Action { none, next_page, save, toggle_rotation, open_pairing, close_pairing, forget_bluetooth, forget_wifi };

class Button {
public:
    explicit Button(bool down = false) : raw(down), stable(down), consumed(down) {}

    Press update(bool down, uint64_t now, bool allow_hold = false) {
        constexpr uint64_t debounce_ms = 30, hold_ms = 800;
        if (raw != down) {
            raw = down;
            changed_at = now;
        }
        if (stable != raw && now - changed_at >= debounce_ms) {
            stable = raw;
            if (stable) {
                pressed_at = changed_at;
                consumed = false;
            } else if (!consumed) {
                return allow_hold && changed_at - pressed_at >= hold_ms ? Press::hold : Press::tap;
            }
        }
        if (allow_hold && stable && raw && !consumed && now - pressed_at >= hold_ms) {
            consumed = true;
            return Press::hold;
        }
        return Press::none;
    }

private:
    bool raw, stable, consumed;
    uint64_t changed_at = 0, pressed_at = 0;
};

struct Menu {
    View view = View::dashboard;
    unsigned selected = 0;
    unsigned brightness = 0;
    bool save_failed = false;
    bool forget_selected = false;

    bool active() const { return view != View::dashboard; }

    unsigned output_brightness(unsigned saved) const {
        if (!active()) return saved;
        return std::max(10u, view == View::brightness ? brightness : saved);
    }

    Action update(Press user, Press boot, unsigned current) {
        if (view == View::dashboard) {
            if (boot == Press::hold) {
                selected = 0;
                save_failed = false;
                view = View::menu;
            } else if (user == Press::tap) {
                return Action::next_page;
            }
        } else if (view == View::menu) {
            if (boot == Press::hold || (boot == Press::tap && selected == 5)) {
                view = View::dashboard;
            } else if (boot == Press::tap) {
                if (selected == 0) {
                    brightness = current;
                    edited = false;
                    view = View::brightness;
                } else if (selected == 1) {
                    return Action::toggle_rotation;
                } else if (selected == 3) {
                    forget_selected = false;
                    view = View::bluetooth;
                    return Action::open_pairing;
                } else if (selected == 4) {
                    forget_selected = false;
                    view = View::wifi;
                } else {
                    view = View::about;
                }
            } else if (user == Press::tap) {
                selected = (selected + 1) % 6;
                save_failed = false;
            }
        } else if (view == View::brightness) {
            if (!edited) brightness = current;
            if (boot == Press::hold) {
                if (brightness != current) return Action::save;
                saved(true);
            } else if (boot == Press::tap || user == Press::tap) {
                if (boot == Press::tap) brightness = std::min(100u, brightness + 10);
                else if (brightness > 10) brightness = std::max(10u, brightness - 10);
                edited = brightness != current;
                save_failed = false;
            }
        } else if (view == View::bluetooth) {
            if (boot == Press::hold) {
                view = View::menu;
                return Action::close_pairing;
            }
            if (user == Press::tap) forget_selected = !forget_selected;
            if (boot == Press::tap) {
                if (forget_selected) {
                    forget_selected = false;
                    return Action::forget_bluetooth;
                }
                return Action::open_pairing;
            }
        } else if (view == View::wifi) {
            if (boot == Press::hold) view = View::menu;
            else if (user == Press::tap) forget_selected = !forget_selected;
            else if (boot == Press::tap && forget_selected) {
                forget_selected = false;
                return Action::forget_wifi;
            }
        } else if (boot != Press::none) {
            view = View::menu;
        }
        return Action::none;
    }

    void saved(bool success) {
        save_failed = !success;
        if (success) view = View::menu;
    }

private:
    bool edited = false;
};
}
