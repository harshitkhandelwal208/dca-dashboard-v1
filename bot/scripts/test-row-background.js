const assert = require("assert");
const path = require("path");
const { classifyNameBox, classifyRgb } = require("../utils/rowBackgroundClassifier");

const fixture = "C:/Users/Harshit/.cursor/projects/c-Users-Harshit-DC-Bot-V2/assets/c__Users_Harshit_AppData_Roaming_Cursor_User_workspaceStorage_empty-window_images_1-5ed34641-bbf4-4523-b390-ad0f865a49a7.png";

async function main() {
    assert.strictEqual(classifyRgb(243, 230, 152), "own");
    assert.strictEqual(classifyRgb(97, 171, 246), "opponent");
    assert.strictEqual(classifyRgb(3, 35, 58), "unknown");

    const ownSample = await classifyNameBox(fixture, { x: 118, y: 808, width: 150, height: 22 });
    const opponentSample = await classifyNameBox(fixture, { x: 118, y: 64, width: 150, height: 22 });

    assert.strictEqual(ownSample.teamType, "own", "expected golden row to classify as own");
    assert.strictEqual(opponentSample.teamType, "opponent", "expected blue row to classify as opponent");

    console.log("Row background classifier test passed.");
}

main().catch(error => {
    console.error(error);
    process.exit(1);
});
