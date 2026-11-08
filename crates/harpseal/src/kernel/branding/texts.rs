//! Default texts of `/admin/branding` (#114), in Thai and English.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(super) const TEXTS: &[E] = th_en![
    ("kernel.branding.title", "แบรนด์และรูปเมนู", "Branding"),
    (
        "kernel.branding.intro",
        "ชื่อ สี และโลโก้ที่แสดงในหลังบ้านและมินิแอป ดูตัวอย่างได้ก่อนบันทึก ทุกการแก้ไขถูกบันทึกในบันทึกการใช้งาน",
        "The name, colour and logo of the back office and the Mini App. Preview before you save; every change is in the audit log."
    ),
    (
        "kernel.branding.look",
        "ชื่อ สี และโลโก้",
        "Name, colour and logo"
    ),
    ("kernel.branding.name", "ชื่อสถาบัน", "Institution name"),
    ("kernel.branding.color", "สีหลัก", "Primary colour"),
    (
        "kernel.branding.color_help",
        "รูปแบบ #RRGGBB เช่น #0b5cad ตัวอักษรสีขาวบนสีนี้ต้องอ่านได้ (คอนทราสต์อย่างน้อย 4.5:1)",
        "#RRGGBB, e.g. #0b5cad. White text on it must stay readable (contrast at least 4.5:1)."
    ),
    ("kernel.branding.logo", "โลโก้", "Logo"),
    (
        "kernel.branding.logo_help",
        "PNG หรือ JPEG ไม่เกิน 256 KB ด้านละ 16 ถึง 1024 px",
        "PNG or JPEG, at most 256 KB, 16 to 1024 px a side."
    ),
    (
        "kernel.branding.logo_drop",
        "ไม่ใช้โลโก้ที่อัปโหลด (กลับไปใช้ค่าจากไฟล์ตั้งค่า)",
        "Drop the uploaded logo (back to the configuration file's)"
    ),
    (
        "kernel.branding.source_toml",
        "ตอนนี้ใช้ค่าจากไฟล์ตั้งค่า",
        "From the configuration file now"
    ),
    (
        "kernel.branding.source_setting",
        "ตอนนี้ใช้ค่าที่บันทึกจากหน้านี้",
        "Saved on this page"
    ),
    ("kernel.branding.preview", "ดูตัวอย่าง", "Preview"),
    (
        "kernel.branding.preview_title",
        "ตัวอย่าง (ยังไม่ได้บันทึก)",
        "Preview (not saved yet)"
    ),
    (
        "kernel.branding.preview_office",
        "แถบบนของหลังบ้าน",
        "Back-office header"
    ),
    (
        "kernel.branding.preview_mini",
        "แถบบนของมินิแอป",
        "Mini App header"
    ),
    ("kernel.branding.save", "บันทึก", "Save"),
    (
        "kernel.branding.reset",
        "คืนค่าตามไฟล์ตั้งค่า",
        "Back to the configuration file"
    ),
    ("kernel.branding.saved", "บันทึกแล้ว", "Saved"),
    (
        "kernel.branding.reset_done",
        "กลับไปใช้ค่าจากไฟล์ตั้งค่าแล้ว",
        "Back to the configuration file's values"
    ),
    (
        "kernel.branding.unchanged",
        "ไม่มีอะไรเปลี่ยน",
        "Nothing changed"
    ),
    (
        "kernel.branding.err_form",
        "กรุณาแก้ไขช่องที่มีข้อผิดพลาด แล้วลองอีกครั้ง",
        "Fix the fields marked below and try again."
    ),
    (
        "kernel.branding.err_name_empty",
        "กรุณาใส่ชื่อสถาบัน",
        "Enter the institution's name."
    ),
    (
        "kernel.branding.err_name_long",
        "ชื่อยาวได้ไม่เกิน {max} ตัวอักษร",
        "At most {max} characters."
    ),
    (
        "kernel.branding.err_name_control",
        "ชื่อมีอักขระที่แสดงไม่ได้",
        "The name holds characters that cannot be shown."
    ),
    (
        "kernel.branding.err_color",
        "สีต้องอยู่ในรูปแบบ #RRGGBB เช่น #0b5cad",
        "Use #RRGGBB, e.g. #0b5cad."
    ),
    (
        "kernel.branding.err_contrast",
        "ตัวอักษรสีขาวบนสีนี้อ่านยาก (คอนทราสต์ {ratio}:1 ต้องอย่างน้อย {min}:1) กรุณาเลือกสีที่เข้มขึ้น",
        "White text on this colour is hard to read (contrast {ratio}:1, at least {min}:1 needed): pick a darker colour."
    ),
    (
        "kernel.branding.err_logo_type",
        "โลโก้ต้องเป็นไฟล์ PNG หรือ JPEG",
        "The logo must be a PNG or JPEG file."
    ),
    (
        "kernel.branding.err_logo_large",
        "โลโก้ใหญ่เกิน {kb} KB",
        "The logo is over {kb} KB."
    ),
    (
        "kernel.branding.err_logo_size",
        "โลโก้ขนาด {w} x {h} px แต่ละด้านต้องอยู่ระหว่าง {min} ถึง {max} px",
        "The logo is {w} x {h} px; each side must be {min} to {max} px."
    ),
    (
        "kernel.branding.menu_title",
        "รูปเมนู LINE (ริชเมนู)",
        "LINE menu images (rich menus)"
    ),
    (
        "kernel.branding.menu_help",
        "ไฟล์ PNG ขนาด 2500 x 843 px (เมนูแถวเดียว) หรือ 2500 x 1686 px (สองแถวขึ้นไป) ไม่เกิน 1 MB วาดปุ่มให้ตรงกับกรอบในตัวอย่าง เมื่อบันทึก ระบบจะสร้างเมนูใหม่บน LINE ก่อน แล้วค่อยเปลี่ยนเมนูของทุกบัญชี",
        "PNG, 2500 x 843 px (one row) or 2500 x 1686 px (two rows or more), at most 1 MB. Draw each tile under its box in the preview. On save the new rich menus are created on LINE first, then every account is moved to them."
    ),
    (
        "kernel.branding.menu_locked",
        "เมนูก่อนเชื่อมบัญชี",
        "Menu before linking"
    ),
    (
        "kernel.branding.menu_unlocked",
        "เมนูหลังเชื่อมบัญชี",
        "Menu after linking"
    ),
    (
        "kernel.branding.menu_configured",
        "ตอนนี้ใช้รูปตั้งต้น",
        "The configured image now"
    ),
    (
        "kernel.branding.menu_saved",
        "ตอนนี้ใช้รูปที่อัปโหลด",
        "An uploaded image now"
    ),
    (
        "kernel.branding.menu_preview",
        "ตรวจรูปและดูตัวอย่าง",
        "Check and preview"
    ),
    (
        "kernel.branding.menu_apply",
        "ใช้รูปนี้กับทุกบัญชี",
        "Use for every account"
    ),
    (
        "kernel.branding.menu_reset",
        "คืนรูปตั้งต้น",
        "Back to the configured images"
    ),
    (
        "kernel.branding.menu_applied",
        "บันทึกแล้ว ระบบกำลังเปลี่ยนเมนูของทุกบัญชีอยู่เบื้องหลัง",
        "Saved. Every account is being moved to the new menu in the background."
    ),
    (
        "kernel.branding.menu_reset_done",
        "กลับไปใช้รูปตั้งต้นแล้ว ระบบกำลังเปลี่ยนเมนูของทุกบัญชีอยู่เบื้องหลัง",
        "Back to the configured images. Every account is being moved in the background."
    ),
    (
        "kernel.branding.menu_none",
        "ระบบนี้ยังไม่มีช่องทางที่ใช้รูปเมนู (เช่น LINE ที่ตั้งค่าแล้ว)",
        "No Channel of this Deployment draws its menu on an image (e.g. a configured LINE account)."
    ),
    (
        "kernel.branding.menu_stage_failed",
        "LINE ไม่รับรูปเมนู ระบบยังไม่ได้บันทึกอะไร และทุกบัญชียังใช้เมนูเดิม",
        "LINE refused the menu images. Nothing was saved and every account keeps its menu."
    ),
    (
        "kernel.branding.menu_pending",
        "กำลังเปลี่ยนเมนูของบัญชีต่าง ๆ ({n} งานค้าง)",
        "Moving accounts to the new menu ({n} jobs pending)"
    ),
    (
        "kernel.branding.menu_areas",
        "กรอบคือพื้นที่กดของแต่ละปุ่ม",
        "Each box is a tap area"
    ),
    (
        "kernel.branding.err_menu_none",
        "กรุณาเลือกรูปอย่างน้อยหนึ่งรูป",
        "Choose at least one image."
    ),
    (
        "kernel.branding.err_menu_type",
        "รูปเมนูต้องเป็นไฟล์ PNG",
        "A menu image must be a PNG file."
    ),
    (
        "kernel.branding.err_menu_large",
        "รูปเมนูใหญ่เกิน {mb} MB",
        "The image is over {mb} MB."
    ),
    (
        "kernel.branding.err_menu_size",
        "รูปขนาด {w} x {h} px แต่ LINE รับเฉพาะ {allowed}",
        "The image is {w} x {h} px; LINE takes only {allowed}."
    ),
    (
        "kernel.branding.err_menu_grid",
        "เมนูนี้มีปุ่ม {rows} แถว รูปจึงต้องขนาด {want} px",
        "This menu has {rows} row(s) of tiles, so the image must be {want} px."
    ),
    (
        "kernel.branding.err_menu_unused",
        "เมนูนี้ไม่มีปุ่ม จึงไม่ได้ใช้รูป",
        "This menu has no entries, so its image is never shown."
    ),
];
