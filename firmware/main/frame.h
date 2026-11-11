#pragma once

#include <cstddef>
#include <cstdint>

class LineFrame {
public:
    LineFrame(char *storage, size_t limit) : storage(storage), limit(limit) {}

    const char *push(uint8_t byte) {
        if (byte == '\n') {
            bool complete = size && !discard;
            storage[size] = '\0';
            reset();
            return complete ? storage : nullptr;
        }
        if (size == limit || byte == 0) discard = true;
        if (!discard) storage[size++] = byte;
        return nullptr;
    }

    bool pending() const { return size || discard; }
    void reset() { size = 0; discard = false; }

private:
    char *storage;
    size_t limit, size = 0;
    bool discard = false;
};
