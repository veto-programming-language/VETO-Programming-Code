#!/usr/bin/env bash
set -e

cargo run --features inquisitor -- kernel.vt --kernel

mkdir -p iso_root/boot/grub
cp veto_kernel.elf iso_root/boot/veto_kernel.elf

cat << 'GRUB' > iso_root/boot/grub/grub.cfg
set timeout=1
set default=0

menuentry "VETO OS v1.1 (Mathematically Verified Ring-0 Kernel)" {
    multiboot /boot/veto_kernel.elf
    boot
}
GRUB

grub-mkrescue -o veto_os.iso iso_root
rm -rf iso_root
echo "[SUCCESS] Bootable ISO image created: ./veto_os.iso"
echo "Boot in QEMU as CD-ROM: qemu-system-i386 -cdrom veto_os.iso -audiodev pa,id=snd0 -machine pcspk-audiodev=snd0"
