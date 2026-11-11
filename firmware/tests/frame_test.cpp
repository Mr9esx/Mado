#include "../main/frame.h"

#include <cassert>
#include <string>
#include <vector>

int main() {
    char storage[9];
    LineFrame frame(storage, sizeof(storage) - 1);
    std::vector<std::string> lines;
    auto push = [&](const std::string &chunk) {
        for (unsigned char byte : chunk)
            if (const char *line = frame.push(byte)) lines.emplace_back(line);
    };
    push("\nabc");
    assert(lines.empty());
    push("defgh\nxy\n");
    assert((lines == std::vector<std::string>{"abcdefgh", "xy"}));
    push("0123456789\n\nok\n");
    assert(lines.size() == 3 && lines.back() == "ok");
    push(std::string("a\0b\n", 4));
    assert(lines.size() == 3);
    push("partial");
    frame.reset();
    push("next\n");
    assert(lines.size() == 4 && lines.back() == "next");
}
