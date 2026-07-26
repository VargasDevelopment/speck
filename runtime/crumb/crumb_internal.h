#ifndef CRUMB_INTERNAL_H
#define CRUMB_INTERNAL_H

enum crumb_present_result {
    CRUMB_PRESENT_ERROR = -1,
    CRUMB_PRESENT_CONTINUE = 0,
    CRUMB_PRESENT_STOP = 1
};

int crumb_present_init(void);
int crumb_present(void);
void crumb_present_shutdown(void);

int crumb_platform_init(void);
int crumb_platform_should_stop(void);
void crumb_platform_shutdown(void);

#endif
