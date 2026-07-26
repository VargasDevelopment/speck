#if !defined(_POSIX_C_SOURCE)
#define _POSIX_C_SOURCE 200809L
#endif

#include "crumb.h"
#include "crumb_internal.h"

#include <signal.h>
#include <stddef.h>

static volatile sig_atomic_t stop_requested = 0;
static struct sigaction previous_sigint_action;
static int interrupt_handler_installed = 0;

static void request_stop(int signal_number) {
    (void)signal_number;
    stop_requested = 1;
}

int crumb_platform_init(void) {
    struct sigaction action = {0};

    stop_requested = 0;
    action.sa_handler = request_stop;
    if (sigemptyset(&action.sa_mask) != 0 ||
        sigaction(SIGINT, &action, &previous_sigint_action) != 0) {
        return 1;
    }
    interrupt_handler_installed = 1;
    return 0;
}

int crumb_platform_should_stop(void) { return stop_requested != 0; }

void crumb_platform_shutdown(void) {
    if (interrupt_handler_installed) {
        (void)sigaction(SIGINT, &previous_sigint_action, NULL);
        interrupt_handler_installed = 0;
    }
}

int main(void) { return crumb_run(); }
