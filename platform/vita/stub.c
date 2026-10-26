// C pieces the Vita's newlib lacks or the app must export (docs/design/vita.md). Compiled by build.sh and linked into
// the program.
#include <errno.h>
#include <stddef.h>

// Rust's std asks for a user's home folder with getpwuid_r, which newlib for the Vita doesn't have. There is no user
// database, so say "no such user"; the program then uses its own folder (see `storage::data_dir`).
struct passwd;
int getpwuid_r(unsigned uid, struct passwd *pw, char *buf, size_t len, struct passwd **res) {
    (void)uid;
    (void)pw;
    (void)buf;
    (void)len;
    *res = NULL;
    return ENOENT;
}

// The size of the main thread's stack, read by the system at start-up (the default is far too small for the program's
// frame buffers). build.sh keeps the symbol alive with `-Wl,-u`.
__attribute__((used)) unsigned int sceUserMainThreadStackSize = 8 * 1024 * 1024;
