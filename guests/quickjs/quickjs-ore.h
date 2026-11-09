#ifndef QUICKJS_ORE_H
#define QUICKJS_ORE_H

#include "quickjs.h"

#ifdef __cplusplus
extern "C" {
#endif

// Injects `globalThis.__ore__` into the QuickJS context
void js_init_ore_bridge(JSContext *ctx);

#ifdef __cplusplus
}
#endif

#endif /* QUICKJS_ORE_H */