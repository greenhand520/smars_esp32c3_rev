CC ?= cc
CFLAGS ?= -std=c99 -Wall -Wextra -Werror -O2

test: test_protocol
	./test_protocol

test_protocol: test_protocol.c ../../src/protocol.c ../../src/protocol.h
	$(CC) $(CFLAGS) -o $@ test_protocol.c ../../src/protocol.c

clean:
	rm -f test_protocol

.PHONY: test clean
