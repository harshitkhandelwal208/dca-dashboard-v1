const fs = require("fs");
const os = require("os");
const path = require("path");
const { analyzeRecruitLicenseText, ocrImage } = require("./raceOcr");

function extensionForAttachment(attachment) {
    const fromName = path.extname(attachment?.name || "").toLowerCase();
    if (/^\.(png|jpe?g|webp|gif)$/i.test(fromName)) return fromName;
    const type = String(attachment?.contentType || "").toLowerCase();
    if (type.includes("png")) return ".png";
    if (type.includes("webp")) return ".webp";
    if (type.includes("gif")) return ".gif";
    return ".jpg";
}

async function downloadAttachment(attachment, dir, index) {
    const response = await fetch(attachment.proxyUrl || attachment.url);
    if (!response.ok) throw new Error(`Could not download license image (${response.status}).`);
    const buffer = Buffer.from(await response.arrayBuffer());
    const filePath = path.join(dir, `license-${index + 1}${extensionForAttachment(attachment)}`);
    await fs.promises.writeFile(filePath, buffer);
    return filePath;
}

async function analyzeRecruitmentLicense(ticket, config) {
    const attachment = (ticket.licenseAttachments || [])[0];
    const base = {
        discordId: ticket.applicantId || "",
        inGameName: "",
        sourceTeam: "",
        acceptedTeam: ticket.team || "",
        rawText: "",
        error: ""
    };

    if (!attachment?.url) {
        return { ...base, error: "No license image was attached to this ticket." };
    }

    const tempDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), "dca-license-"));
    try {
        const imagePath = await downloadAttachment(attachment, tempDir, 0);
        const ocr = await ocrImage(imagePath, {
            tesseractPath: config?.spreadsheets?.tesseractPath,
            imageMagickPath: config?.spreadsheets?.imageMagickPath,
            tesseractLang: config?.spreadsheets?.tesseractLang,
            tesseractPsm: config?.spreadsheets?.tesseractPsm || "6,11"
        });
        const parsed = analyzeRecruitLicenseText(ocr.text, config);
        return {
            ...base,
            inGameName: parsed.inGameName || "",
            sourceTeam: parsed.sourceTeam || "",
            rawText: parsed.rawText || ocr.text || ""
        };
    } catch (error) {
        return { ...base, error: error.message };
    } finally {
        await fs.promises.rm(tempDir, { recursive: true, force: true }).catch(() => null);
    }
}

module.exports = {
    analyzeRecruitmentLicense
};
