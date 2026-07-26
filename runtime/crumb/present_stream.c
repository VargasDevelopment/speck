#if !defined(_POSIX_C_SOURCE)
#define _POSIX_C_SOURCE 200112L
#endif

#include "crumb.h"
#include "crumb_internal.h"

#include <arpa/inet.h>
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <unistd.h>

enum {
    CRUMB_FRAME_HEADER_BYTES = 24,
    CRUMB_FRAME_PROTOCOL_VERSION = 1,
    CRUMB_PIXEL_FORMAT_RGB8 = 1
};

static int stream_socket = -1;
static uint64_t frame_sequence = 0;

static void write_u16(unsigned char *target, uint16_t value) {
    target[0] = (unsigned char)(value >> 8);
    target[1] = (unsigned char)value;
}

static void write_u32(unsigned char *target, uint32_t value) {
    target[0] = (unsigned char)(value >> 24);
    target[1] = (unsigned char)(value >> 16);
    target[2] = (unsigned char)(value >> 8);
    target[3] = (unsigned char)value;
}

static void write_u64(unsigned char *target, uint64_t value) {
    int index;

    for (index = 7; index >= 0; --index) {
        target[index] = (unsigned char)value;
        value >>= 8;
    }
}

static int send_all(const unsigned char *bytes, size_t length) {
    size_t sent = 0;

    while (sent < length) {
        ssize_t result;
#ifdef MSG_NOSIGNAL
        result = send(stream_socket, bytes + sent, length - sent, MSG_NOSIGNAL);
#else
        result = send(stream_socket, bytes + sent, length - sent, 0);
#endif
        if (result < 0 && errno == EINTR) {
            continue;
        }
        if (result <= 0) {
            return 1;
        }
        sent += (size_t)result;
    }
    return 0;
}

int crumb_present_init(void) {
    const char *port_text = getenv("SPECK_FRAME_STREAM_PORT");
    struct sockaddr_in address = {0};
    char *end = NULL;
    long port;

    if (port_text == NULL) {
        return 1;
    }
    errno = 0;
    port = strtol(port_text, &end, 10);
    if (errno != 0 || end == port_text || *end != '\0' || port <= 0 || port > 65535) {
        return 1;
    }

    stream_socket = socket(AF_INET, SOCK_STREAM, 0);
    if (stream_socket < 0) {
        return 1;
    }
#ifdef SO_NOSIGPIPE
    {
        int enabled = 1;
        if (setsockopt(stream_socket, SOL_SOCKET, SO_NOSIGPIPE, &enabled, sizeof(enabled)) != 0) {
            close(stream_socket);
            stream_socket = -1;
            return 1;
        }
    }
#endif

    address.sin_family = AF_INET;
    address.sin_port = htons((uint16_t)port);
    if (inet_pton(AF_INET, "127.0.0.1", &address.sin_addr) != 1 ||
        connect(stream_socket, (struct sockaddr *)&address, sizeof(address)) != 0) {
        close(stream_socket);
        stream_socket = -1;
        return 1;
    }
    frame_sequence = 0;
    return CRUMB_PRESENT_CONTINUE;
}

int crumb_present(void) {
    unsigned char header[CRUMB_FRAME_HEADER_BYTES] = {'S', 'P', 'K', 'F'};

    if (stream_socket < 0) {
        return CRUMB_PRESENT_ERROR;
    }
    ++frame_sequence;
    header[4] = CRUMB_FRAME_PROTOCOL_VERSION;
    header[5] = CRUMB_PIXEL_FORMAT_RGB8;
    write_u16(header + 6, CRUMB_FRAME_HEADER_BYTES);
    write_u16(header + 8, CRUMB_FRAMEBUFFER_WIDTH);
    write_u16(header + 10, CRUMB_FRAMEBUFFER_HEIGHT);
    write_u32(header + 12, CRUMB_FRAMEBUFFER_BYTES);
    write_u64(header + 16, frame_sequence);

    if (send_all(header, sizeof(header)) != 0 ||
        send_all(crumb_framebuffer_pixels(), CRUMB_FRAMEBUFFER_BYTES) != 0) {
        return CRUMB_PRESENT_ERROR;
    }
    return CRUMB_PRESENT_CONTINUE;
}

void crumb_present_shutdown(void) {
    if (stream_socket >= 0) {
        close(stream_socket);
        stream_socket = -1;
    }
}
