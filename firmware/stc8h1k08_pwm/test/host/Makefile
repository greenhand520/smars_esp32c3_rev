CC ?= cc
CFLAGS ?= -std=c99 -Wall -Wextra -Werror -O2

TESTS = test_protocol test_pwm_math

test: $(TESTS)
	./test_protocol
	./test_pwm_math

test_protocol: test_protocol.c ../../src/protocol.c ../../src/protocol.h
	$(CC) $(CFLAGS) -o $@ test_protocol.c ../../src/protocol.c

test_pwm_math: test_pwm_math.c ../../src/pwm_math.h
	$(CC) $(CFLAGS) -o $@ test_pwm_math.c

clean:
	rm -f $(TESTS)

.PHONY: test clean
