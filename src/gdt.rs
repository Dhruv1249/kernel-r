use spin::Once;
use x86_64::structures::gdt::SegmentSelector;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
const STACK_SIZE: usize = 4096 * 16;
pub static mut DOUBLE_FAULT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

struct Gdt {
    table: GlobalDescriptorTable,
    code_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

static TSS_ONCE: Once<TaskStateSegment> = Once::new();
static GDT_ONCE: Once<Gdt> = Once::new();

pub fn init() {
    use crate::serial_println;
    use x86_64::instructions::segmentation::{Segment, CS};
    use x86_64::instructions::tables::load_tss;

    let tss = TSS_ONCE.call_once(|| {
        let mut tss = TaskStateSegment::new();
        let stack_start = VirtAddr::from_ptr(unsafe { &raw const DOUBLE_FAULT_STACK });
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_start + STACK_SIZE;
        serial_println!("TSS IST[{}]: {:#x}", DOUBLE_FAULT_IST_INDEX, (stack_start + STACK_SIZE).as_u64());
        tss
    });

    let gdt = GDT_ONCE.call_once(|| {
        let mut table = GlobalDescriptorTable::new();
        let code_selector = table.add_entry(Descriptor::kernel_code_segment());
        let tss_selector = table.add_entry(Descriptor::tss_segment(tss));
        Gdt { table, code_selector, tss_selector }
    });

    gdt.table.load();
    unsafe {
        load_tss(gdt.tss_selector);
        CS::set_reg(gdt.code_selector);
    }
    serial_println!("GDT init complete");
}

pub fn get_ist0() -> u64 {
    TSS_ONCE.call_once(|| panic!("TSS not initialized"))
        .interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize]
        .as_u64()
}
