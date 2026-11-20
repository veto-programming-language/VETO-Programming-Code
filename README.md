# Veto (`vetoc`) v1.1 — Verified Bare-Metal Systems Language

> **Строже, чем Rust. Без сборщика мусора. Ноль проверок в рантайме.**  
> Системный компилируемый язык программирования со структурами (`struct`), строгими паспортными диапазонами полей и переменных (`i32<min..max>`, `u8<0..255>`), модульной системой (`use`) и встроенным **ИИ-Инквизитором (`prove(...)`)**.

---

## Сборка загрузочного `.iso`-образа (`VETO OS v1.1`)

### Зависимости (Ubuntu / Debian)
```bash
sudo apt update && sudo apt install -y build-essential gcc-multilib qemu-system-x86 grub-pc-bin xorriso mtools cargo rustc
```

### Генерация `veto_os.iso` (GRUB) и запуск
```bash
./build_iso.sh
qemu-system-i386 -cdrom veto_os.iso -audiodev pa,id=snd0 -machine pcspk-audiodev=snd0
```
*(Образ `veto_os.iso` также можно напрямую подключить в VirtualBox, VMware или записать на USB-флешку через Rufus / `dd` для загрузки на реальном ПК).*

### Команды в консоли ядра `veto@ring0:~$`
* `help` — справка по ядру и архитектуре
* `mem` — проверка записи и чтения физической памяти (`0x200000`)
* `beep` — аппаратный звуковой сигнал 1000 Гц через таймер PIT канал 2 и порт `0x61`
* `ticks` — вывод счётчика аппаратных прерываний таймера
* `clear` — очистка экрана VGA
* `reboot` — аппаратная перезагрузка процессора через контроллер `0x64`

---

## Синтаксис Veto v1.1

* **Структуры с контрактами полей:**
  ```veto
  struct KernelState {
      row: i32<5..23>,
      cmd_len: i32<0..32>,
      ram_ok: u8<0..1>,
  }
  ```
* **Модули и константы:** `use "drivers.vt";`, `const VGA_CELLS: i32<0..2000> = 2000;`
* **Логика и циклы:** `&&`, `||`, `!`, `+=`, `-=`, `&=`, `|=`, `while`, `loop`, `break`, `continue`
* **Формальная верификация:** `prove(условие);` с точной диагностикой по номерам строк (`[VETO | строка N]`)
