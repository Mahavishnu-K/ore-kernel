#include <ore.h>

// A tiny stateful plugin used to verify the ore-ld lifecycle (dlclose + slab recycling).
//
// `calls` lives in .bss: it must start at 0 every time the plugin is loaded,
// even when ore-ld hands it a recycled memory slab that a previous instance dirtied.
// `step` lives in .data: its initial value is re-applied from the data segment on every load.
static int calls;
static int step = 1;

ORE_PLUGIN int counter_next(void) {
    calls += step;
    return calls;
}

// Exposes where the plugin's state lives, so the host can prove the slab was reused.
ORE_PLUGIN int* counter_state_addr(void) {
    return &calls;
}
