#include <ore.h>

// The ORE_PLUGIN macro now automatically handles `extern "C"` under the hood
ORE_PLUGIN int sanitize_spaces(char* text, int len) {
    int write_idx = 0;
    bool in_space = false;
    
    for(int i = 0; i < len; i++) {
        if (text[i] == ' ') {
            if (!in_space) {
                text[write_idx++] = ' ';
                in_space = true;
            }
        } else {
            text[write_idx++] = text[i];
            in_space = false;
        }
    }
    text[write_idx] = '\0'; 
    return write_idx;
}