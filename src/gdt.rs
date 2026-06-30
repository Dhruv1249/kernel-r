// src/gdt.rs

use spin::Once;
use x86_64::VirtAddr;
use x86_64::structures::gdt::SegmentSelector;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::tss::TaskStateSegment;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
const STACK_SIZE: usize = 4096 * 16;

// Force the compiler to 16-byte align the stack array
#[repr(align(16))]
pub struct Stack {
    buffer: [u8; STACK_SIZE],
}

static DOUBLE_FAULT_STACK: Stack = Stack {
    buffer: [0; STACK_SIZE],
};

static PRIVILEGE_STACK: Stack = Stack {
    buffer: [0; STACK_SIZE],
};

// Force the compiler to 16-byte align the TSS
#[repr(align(16))]
struct AlignedTss(TaskStateSegment);
struct Gdt {
    table: GlobalDescriptorTable,
    code_selector: SegmentSelector,
    tss_selector: SegmentSelector,
    user_data: SegmentSelector,
    user_code: SegmentSelector,
}

static TSS_ONCE: Once<AlignedTss> = Once::new();
static GDT_ONCE: Once<Gdt> = Once::new();

pub fn user_data_selector() -> SegmentSelector {
    GDT_ONCE.r#try().expect("GDT not initialized").user_data
}

pub fn user_code_selector() -> SegmentSelector {
    GDT_ONCE.r#try().expect("GDT not initialized").user_code
}

pub fn init() {
    use crate::serial_println;
    use x86_64::instructions::segmentation::{CS, Segment};
    use x86_64::instructions::tables::load_tss;

    let tss = TSS_ONCE.call_once(|| {
        let mut tss = TaskStateSegment::new();
        let stack_start = VirtAddr::from_ptr({ &raw const DOUBLE_FAULT_STACK.buffer });
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_start + STACK_SIZE;
        let priv_stack_start = VirtAddr::from_ptr({ &raw const PRIVILEGE_STACK.buffer });
        tss.privilege_stack_table[0] = priv_stack_start + STACK_SIZE;
        serial_println!(
            "TSS IST[{}]: {:#x}",
            DOUBLE_FAULT_IST_INDEX,
            (stack_start + STACK_SIZE).as_u64()
        );
        AlignedTss(tss)
    });

    let gdt = GDT_ONCE.call_once(|| {
        let mut table = GlobalDescriptorTable::new();
        let code_selector = table.add_entry(Descriptor::kernel_code_segment());
        let tss_selector = table.add_entry(Descriptor::tss_segment(&tss.0));
        let user_data = table.add_entry(Descriptor::user_data_segment());
        let user_code = table.add_entry(Descriptor::user_code_segment());
        Gdt {
            table,
            code_selector,
            tss_selector,
            user_data,
            user_code,
        }
    });

    gdt.table.load();
    unsafe {
        CS::set_reg(gdt.code_selector);
        load_tss(gdt.tss_selector);
    }
    serial_println!("GDT init complete");
}

pub unsafe fn jump_to_user_space(code_addr: u64, stack_addr: u64) -> ! {
    let user_data = crate::gdt::user_data_selector().0 as u64;
    let user_code = crate::gdt::user_code_selector().0 as u64;

    // Define our RFLAGS
    let rflags = 0x202u64;

    // Fake the interrupt frame and return
    unsafe {
        core::arch::asm!(
            "push {ss}",
            "push {rsp}",
            "push {rflags}",
            "push {cs}",
            "push {rip}",
            "iretq",
            ss = in(reg) user_data,
            rsp = in(reg) stack_addr,
            rflags = in(reg) rflags,
            cs = in(reg) user_code,
            rip = in(reg) code_addr,
            options(noreturn)
        );
    }
}
