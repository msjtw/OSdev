set architecture riscv:rv32
set logging file qemu.log
set logging enabled on
target remote :1234
symbol-file ~/Documents/osdev/rust-baremetal/target/riscv32ima-unknown-none-elf/debug/main
dashboard -layout variables stack
layout asm
layout split 
break main
break *0x80207cb0
break virtmemory.rs:515
break main::virtmemory::copy_in_bytes
disp/x $sp
disp/x $scause


while 1
    si
    x/i $pc
    printf "SP=%p\n", $sp

    if $pc == 0x80207cb0
        break
    end
end
