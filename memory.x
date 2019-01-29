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

  .local (NOLOAD) : {
    /* place stack at base of RAM to catch overflow */
    . += 1536;
    _stack_start = .;
    /* allow things to be placed here */
    *(.local_ram)
    . = ALIGN(4);

    _arena_ccm_start = .;
    . = ORIGIN(CCM) + LENGTH(CCM);
    _arena_ccm_end = .;
  } >CCM

  .sram16 (NOLOAD) : {
    *(.scanout_ram)
  } > SRAM16

} INSERT AFTER .bss;

SECTIONS {
  .not_at_zero (NOLOAD) : {
    /* bump location counter to avoid placing anything at zero */
    . += 4;
  } >RAM
} INSERT BEFORE .data;

__vector_table_in_flash = ADDR(.vector_table);
