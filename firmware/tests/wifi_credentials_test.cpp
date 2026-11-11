#include "../main/wifi_credentials.h"
#include <cassert>
#include <string>

int main() {
    using wifi::valid_credentials;
    assert(valid_credentials("Mado Lab", "a good passphrase"));
    assert(valid_credentials(std::string(32, 's'), ""));
    assert(valid_credentials("Mado Lab", std::string(64, 'f')));
    assert(!valid_credentials("", "validpass"));
    assert(!valid_credentials(std::string(33, 's'), "validpass"));
    assert(!valid_credentials("Mado Lab", "short"));
    assert(!valid_credentials("Mado Lab", std::string(64, 'x')));
    assert(!valid_credentials(std::string("A\0B", 3), "validpass"));
    assert(!valid_credentials("Mado Lab", "valid\npass"));
}
