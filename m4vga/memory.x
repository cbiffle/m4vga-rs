MEMORY
{
  /* NOTE K = KiBi = 1024 bytes */
  FLASH  (rx)  : ORIGIN = 0x08000000, LENGTH = 512K 
  RAM    (rwx) : ORIGIN = 0x00000000, LENGTH = 112K
  CCM    (rw)  : ORIGIN = 0x10000000, LENGTH =  64K
  SRAM16 (rwx) : ORIGIN = 0x2001c000, LENGTH =  16K
}

SECTIONS {
  .arena_sram1 (NOLOAD) : {
    . = ALIGN(4);
    _arena_sram1_start = .;
    /* exhaust the rest of this SRAM */
    . = ORIGIN(RAM) + LENGTH(RAM);
    _arena_sram1_end = .;
  } >RAM

  .local_stack (NOLOAD) : ALIGN(4) {
    /* place stack at base of RAM to catch overflow */
    . += 2048;
    _stack_start = .;
  } >CCM

  .local_data : ALIGN(4) {
    *(.local_data)
    . = ALIGN(4);
  } >CCM AT>FLASH

  _local_data_start = ADDR(.local_data);
  _local_data_end = ADDR(.local_data) + SIZEOF(.local_data);
  _local_data_init = LOADADDR(.local_data);

  .local_bss (NOLOAD) : ALIGN(4) {
    *(.local_bss)
    . = ALIGN(4);
    _arena_ccm_start = .;
    . = ORIGIN(CCM) + LENGTH(CCM);
    _arena_ccm_end = .;
  } >CCM

  _local_bss_start = ADDR(.local_bss);
  _local_bss_end = ADDR(.local_bss) + SIZEOF(.local_bss);

  .sram16 (NOLOAD) : {
    *(.scanout_bss)
  } > SRAM16

  _sram16_bss_start = ADDR(.sram16);
  _sram16_bss_end = ADDR(.sram16) + SIZEOF(.sram16);
} INSERT AFTER .bss;

SECTIONS {
  .not_at_zero (NOLOAD) : {
    /* bump location counter to avoid placing anything at zero */
    . += 4;
  } >RAM
} INSERT BEFORE .data;

__vector_table_in_flash = ADDR(.vector_table);
