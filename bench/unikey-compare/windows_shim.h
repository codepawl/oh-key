// Force-included while compiling the UniKey sources on Windows.
// convert.cpp calls the POSIX mkstemp, but only in its file-conversion helper,
// which this benchmark never calls, so a stub that always fails is enough.
#pragma once

inline int mkstemp(char*) { return -1; }
