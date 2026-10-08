TOOLPREFIX = riscv32-unknown-linux-musl-

CC = $(TOOLPREFIX)gcc
OBJCOPY = $(TOOLPREFIX)objcopy
OBJDUMP = $(TOOLPREFIX)objdump

CFLAGS = -Wall -Werror -Wno-unknown-attributes
CFLAGS += -O
CFLAGS += -fno-omit-frame-pointer
CFLAGS += -ggdb -gdwarf-2
CFLAGS += -MD
CFLAGS += -mcmodel=medany
CFLAGS += -I.
CFLAGS += -fno-stack-protector

# Disable PIE
CFLAGS += -fno-pie

LDFLAGS = -static
LDFLAGS += -no-pie

UPROGS = \
	_prime \
	_init \
	_hello \
	_pipe1 \
	_pipe2 \
	_fork_prime

all: $(UPROGS)

_%: %.o user.ld
	$(CC) $(LDFLAGS) -T user.ld -o $@ $<
	$(OBJDUMP) -S $@ > $*.asm
	$(OBJDUMP) -t $@ | sed '1,/SYMBOL TABLE/d; s/ .* / /; /^$$/d' > $*.sym
	$(OBJCOPY) -O binary $@ $@.bin

clean:
	rm -f *.o *.d *.asm *.sym *.bin $(UPROGS)

-include $(wildcard *.d)

.PRECIOUS: %.o
