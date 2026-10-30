//! ELF64 little-endian reader (frozen).
//!
//! Reads the subset defined in `spec/elf-corpus-v1.md`, section 2: the file
//! header, the section header table, `SHT_SYMTAB`, `SHT_STRTAB` and `SHT_RELA`
//! sections. Everything is exposed as plain data; nothing here interprets a
//! relocation.

pub const ET_REL: u16 = 1;
pub const ET_EXEC: u16 = 2;
pub const EM_VX: u16 = 0x5658;
pub const SHT_NULL: u32 = 0;
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHN_UNDEF: u16 = 0;
pub const SHN_ABS: u16 = 0xfff1;

#[derive(Clone, Debug)]
pub struct Section {
    pub index: usize,
    pub name: String,
    pub sh_type: u32,
    pub flags: u64,
    pub addr: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub info: u32,
    pub addralign: u64,
    pub entsize: u64,
    /// Section contents (empty for `SHT_NULL`).
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub index: usize,
    pub name: String,
    pub value: u64,
    pub size: u64,
    pub info: u8,
    pub other: u8,
    pub shndx: u16,
}

#[derive(Clone, Debug)]
pub struct Rela {
    /// Index of the section this entry relocates (`sh_info` of the RELA section).
    pub section: usize,
    /// Index of the RELA section the entry was read from.
    pub rela_section: usize,
    /// Position of the entry within its RELA section.
    pub entry: usize,
    pub offset: u64,
    /// Symbol table index (upper 32 bits of `r_info`).
    pub symbol: u32,
    /// Relocation type (lower 32 bits of `r_info`).
    pub r_type: u32,
    pub addend: i64,
}

#[derive(Clone, Debug)]
pub struct ElfFile {
    pub e_type: u16,
    pub e_machine: u16,
    pub sections: Vec<Section>,
    pub symbols: Vec<Symbol>,
    pub relocations: Vec<Rela>,
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| format!("truncated at byte {}", at))
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| format!("truncated at byte {}", at))
}
fn u64_at(b: &[u8], at: usize) -> Result<u64, String> {
    b.get(at..at + 8).map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]])).ok_or_else(|| format!("truncated at byte {}", at))
}

fn cstr(table: &[u8], at: usize) -> Result<String, String> {
    let rest = table.get(at..).ok_or("string offset out of range")?;
    let end = rest.iter().position(|&c| c == 0).ok_or("unterminated string")?;
    String::from_utf8(rest[..end].to_vec()).map_err(|_| "non-UTF-8 name".to_string())
}

impl ElfFile {
    pub fn parse(bytes: &[u8]) -> Result<ElfFile, String> {
        if bytes.len() < 64 || &bytes[0..4] != b"\x7fELF" {
            return Err("not an ELF file".into());
        }
        if bytes[4] != 2 { return Err("not ELFCLASS64".into()); }
        if bytes[5] != 1 { return Err("not little-endian".into()); }
        let e_type = u16_at(bytes, 16)?;
        let e_machine = u16_at(bytes, 18)?;
        let e_shoff = u64_at(bytes, 40)? as usize;
        let e_shentsize = u16_at(bytes, 58)? as usize;
        let e_shnum = u16_at(bytes, 60)? as usize;
        let e_shstrndx = u16_at(bytes, 62)? as usize;
        if e_shentsize != 64 { return Err("unexpected section header size".into()); }
        let mut raw = Vec::with_capacity(e_shnum);
        for i in 0..e_shnum {
            let h = e_shoff + 64 * i;
            raw.push((
                u32_at(bytes, h)?, u32_at(bytes, h + 4)?, u64_at(bytes, h + 8)?, u64_at(bytes, h + 16)?,
                u64_at(bytes, h + 24)?, u64_at(bytes, h + 32)?, u32_at(bytes, h + 40)?, u32_at(bytes, h + 44)?,
                u64_at(bytes, h + 48)?, u64_at(bytes, h + 56)?,
            ));
        }
        let shstr = raw.get(e_shstrndx).ok_or("bad e_shstrndx")?;
        let shstr_data = bytes.get(shstr.4 as usize..(shstr.4 + shstr.5) as usize).ok_or("section name table out of range")?;
        let mut sections = Vec::with_capacity(e_shnum);
        for (index, r) in raw.iter().enumerate() {
            let data = if r.1 == SHT_NULL || r.1 == 8 {
                Vec::new()
            } else {
                bytes.get(r.4 as usize..(r.4 + r.5) as usize).ok_or_else(|| format!("section {} out of range", index))?.to_vec()
            };
            sections.push(Section {
                index, name: cstr(shstr_data, r.0 as usize)?, sh_type: r.1, flags: r.2, addr: r.3, offset: r.4,
                size: r.5, link: r.6, info: r.7, addralign: r.8, entsize: r.9, data,
            });
        }
        let mut symbols = Vec::new();
        for s in &sections {
            if s.sh_type == SHT_SYMTAB {
                let strtab = sections.get(s.link as usize).ok_or("symbol table has no string table")?;
                if s.entsize != 24 { return Err("unexpected symbol entry size".into()); }
                for j in 0..(s.size / 24) as usize {
                    let e = &s.data[24 * j..24 * j + 24];
                    symbols.push(Symbol {
                        index: j,
                        name: cstr(&strtab.data, u32_at(e, 0)? as usize)?,
                        info: e[4], other: e[5], shndx: u16_at(e, 6)?, value: u64_at(e, 8)?, size: u64_at(e, 16)?,
                    });
                }
            }
        }
        let mut relocations = Vec::new();
        for s in &sections {
            if s.sh_type == SHT_RELA {
                if s.entsize != 24 { return Err("unexpected RELA entry size".into()); }
                if s.info as usize >= sections.len() { return Err("RELA section relocates a missing section".into()); }
                for j in 0..(s.size / 24) as usize {
                    let e = &s.data[24 * j..24 * j + 24];
                    let info = u64_at(e, 8)?;
                    relocations.push(Rela {
                        section: s.info as usize, rela_section: s.index, entry: j,
                        offset: u64_at(e, 0)?, symbol: (info >> 32) as u32, r_type: (info & 0xffff_ffff) as u32,
                        addend: u64_at(e, 16)? as i64,
                    });
                }
            }
        }
        Ok(ElfFile { e_type, e_machine, sections, symbols, relocations })
    }

    pub fn section_named(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }
}
