// Replays the oh-key comparison corpus through UniKey's ukengine and times it.
//
//   unikey_bench <keys-file> <text-out> <timing-out>
//
// Options set on UniKey (everything else keeps its default from the library):
//   input method        Telex
//   output charset      XUTF8 (Unicode, backspace counts are characters)
//   freeMarking         1  (oh-key: free_tone_placement = true)
//   modernStyle         1  (oh-key: ToneStyle::Modern)
//   spellCheckEnabled   1  (oh-key: spell_check = true)
//   autoNonVnRestore    1  (oh-key: restore_invalid_word = true)
//   macroEnabled        0  (oh-key: macro_enabled = false)
//   useUnicodeClipboard 0, alwaysMacro 0 (not used by the engine core)

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <fstream>
#include <iostream>
#include <iterator>
#include <string>
#include <vector>

#include "unikey.h"
#include "vnconv.h"

#include <windows.h>

namespace {

const int kWarmupReplays = 3;
const int kTimedReplays = 20;

std::u32string g_text;

void configureUniKey() {
  UnikeySetup();
  UnikeySetInputMethod(UkTelex);
  UnikeySetOutputCharset(CONV_CHARSET_XUTF8);
  UnikeyOptions options;
  CreateDefaultUnikeyOptions(&options);
  options.freeMarking = 1;
  options.modernStyle = 1;
  options.spellCheckEnabled = 1;
  options.autoNonVnRestore = 1;
  options.macroEnabled = 0;
  UnikeySetOptions(&options);
}

// Decodes UTF-8 bytes produced by the engine and appends the code points.
void appendUtf8(const unsigned char* bytes, int byteCount) {
  int index = 0;
  while (index < byteCount) {
    unsigned char lead = bytes[index];
    int extraBytes = lead >= 0xF0 ? 3 : lead >= 0xE0 ? 2 : lead >= 0xC0 ? 1 : 0;
    char32_t codePoint = extraBytes == 0 ? lead : lead & (0x3F >> extraBytes);
    for (int offset = 1; offset <= extraBytes && index + offset < byteCount; offset++) {
      codePoint = (codePoint << 6) | (bytes[index + offset] & 0x3F);
    }
    g_text.push_back(codePoint);
    index += 1 + extraBytes;
  }
}

void eraseLastCharacters(int count) {
  size_t keep = g_text.size() > static_cast<size_t>(count) ? g_text.size() - count : 0;
  g_text.resize(keep);
}

// What a frontend does for one key: filter it, delete `backs` characters, then type
// the engine's output, or type the key itself when the engine produced nothing.
void typeKey(unsigned char key) {
  if (key == '\n') {
    UnikeyResetBuf();
    g_text.push_back(key);
    return;
  }
  UnikeySetCapsState(key >= 'A' && key <= 'Z', 0);
  UnikeyFilter(key);
  if (UnikeyBackspaces > 0 || UnikeyBufChars > 0) {
    eraseLastCharacters(UnikeyBackspaces);
    appendUtf8(UnikeyBuf, UnikeyBufChars);
  } else {
    g_text.push_back(key);
  }
}

void replay(const std::string& keys) {
  g_text.clear();
  UnikeyResetBuf();
  for (unsigned char key : keys) {
    typeKey(key);
  }
}

double nanosecondsPerKey(const std::string& keys, double ticksPerSecond) {
  LARGE_INTEGER started, finished;
  QueryPerformanceCounter(&started);
  replay(keys);
  QueryPerformanceCounter(&finished);
  double seconds = (finished.QuadPart - started.QuadPart) / ticksPerSecond;
  return seconds * 1e9 / keys.size();
}

double median(const std::vector<double>& sorted) {
  size_t middle = sorted.size() / 2;
  if (sorted.size() % 2 == 0) {
    return (sorted[middle - 1] + sorted[middle]) / 2.0;
  }
  return sorted[middle];
}

double percentile(const std::vector<double>& sorted, int percent) {
  return sorted[(sorted.size() - 1) * percent / 100];
}

std::string encodeUtf8(const std::u32string& text) {
  std::string encoded;
  for (char32_t codePoint : text) {
    if (codePoint < 0x80) {
      encoded.push_back(static_cast<char>(codePoint));
    } else if (codePoint < 0x800) {
      encoded.push_back(static_cast<char>(0xC0 | (codePoint >> 6)));
      encoded.push_back(static_cast<char>(0x80 | (codePoint & 0x3F)));
    } else if (codePoint < 0x10000) {
      encoded.push_back(static_cast<char>(0xE0 | (codePoint >> 12)));
      encoded.push_back(static_cast<char>(0x80 | ((codePoint >> 6) & 0x3F)));
      encoded.push_back(static_cast<char>(0x80 | (codePoint & 0x3F)));
    } else {
      encoded.push_back(static_cast<char>(0xF0 | (codePoint >> 18)));
      encoded.push_back(static_cast<char>(0x80 | ((codePoint >> 12) & 0x3F)));
      encoded.push_back(static_cast<char>(0x80 | ((codePoint >> 6) & 0x3F)));
      encoded.push_back(static_cast<char>(0x80 | (codePoint & 0x3F)));
    }
  }
  return encoded;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc != 4) {
    std::fprintf(stderr, "usage: unikey_bench <keys-file> <text-out> <timing-out>\n");
    return 2;
  }
  std::ifstream keysFile(argv[1], std::ios::binary);
  std::string keys((std::istreambuf_iterator<char>(keysFile)), std::istreambuf_iterator<char>());
  if (keys.empty()) {
    std::fprintf(stderr, "empty or unreadable keys file\n");
    return 1;
  }
  configureUniKey();
  g_text.reserve(keys.size() * 2);

  LARGE_INTEGER frequency;
  QueryPerformanceFrequency(&frequency);
  for (int index = 0; index < kWarmupReplays; index++) {
    replay(keys);
  }
  std::vector<double> samples;
  for (int index = 0; index < kTimedReplays; index++) {
    samples.push_back(nanosecondsPerKey(keys, static_cast<double>(frequency.QuadPart)));
  }
  std::sort(samples.begin(), samples.end());

  std::ofstream textFile(argv[2], std::ios::binary);
  textFile << encodeUtf8(g_text);
  std::ofstream timingFile(argv[3]);
  char line[128];
  std::snprintf(line, sizeof(line), "%.2f %.2f %.2f\n", median(samples), percentile(samples, 10),
                percentile(samples, 90));
  timingFile << line;
  std::printf("UniKey replayed %zu keys %d times\n", keys.size(), kTimedReplays);
  return 0;
}
