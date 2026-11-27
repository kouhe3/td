// Oracle for AuthData::next_seq_no / next_message_id.
//
// msgid_probe <now_seconds> <count>
//   -> one line per call: seq=<int> msg_id=<uint64> content=<0|1>
//
// `next_seq_no` is a deterministic state machine. `next_message_id` mixes the
// server time with Random::secure_int32, so only its high bits, its %4 == 0
// invariant and monotonicity are comparable across implementations.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils tools/fixtures/msgid_probe.cpp \
//       -Wl,--start-group C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -liphlpapi -o msgid_probe.exe

#include "td/mtproto/AuthData.h"

#include "td/utils/common.h"

#include <cstdio>
#include <cstdlib>

int main(int argc, char **argv) {
  if (argc < 3) {
    std::fprintf(stderr, "usage: msgid_probe <now_seconds> <count>\n");
    return 2;
  }
  double now = std::strtod(argv[1], nullptr);
  int count = std::atoi(argv[2]);

  td::mtproto::AuthData data;
  for (int i = 0; i < count; i++) {
    bool content = (i % 2) == 0;
    td::int32 seq = data.next_seq_no(content);
    td::uint64 msg_id = data.next_message_id(now).get();
    std::printf("seq=%d msg_id=%llu content=%d\n", seq, static_cast<unsigned long long>(msg_id), content ? 1 : 0);
  }
  return 0;
}