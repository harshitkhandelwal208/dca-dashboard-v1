const fs = require("fs");
const path = require("path");

const OWN_COLOR = "yellow";
const OPPONENT_COLOR = "blue";

function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
}

function parseCoordinate(value) {
    const number = Number(value);
    return Number.isFinite(number) ? number : null;
}

function normalizeNameBox(raw, imageWidth, imageHeight) {
    if (!raw || !imageWidth || !imageHeight) return null;

    const box = raw.nameBox || raw.name_box || raw.boundingBox || raw.bbox || null;
    const center = raw.nameCenter || raw.name_center || raw.center || null;

    let x = null;
    let y = null;
    let width = null;
    let height = null;

    if (box && typeof box === "object") {
        x = parseCoordinate(box.x ?? box.left);
        y = parseCoordinate(box.y ?? box.top);
        width = parseCoordinate(box.width ?? box.w);
        height = parseCoordinate(box.height ?? box.h);
    } else if (Array.isArray(box) && box.length >= 4) {
        [x, y, width, height] = box.map(parseCoordinate);
    }

    if (center && typeof center === "object") {
        const cx = parseCoordinate(center.x);
        const cy = parseCoordinate(center.y);
        if (cx !== null && cy !== null) {
            const defaultWidth = Math.max(48, Math.round(imageWidth * 0.28));
            const defaultHeight = Math.max(12, Math.round(imageHeight * 0.018));
            x = cx - Math.round(defaultWidth / 2);
            y = cy - Math.round(defaultHeight / 2);
            width = defaultWidth;
            height = defaultHeight;
        }
    }

    if ([x, y, width, height].some(value => value === null)) return null;

    const usesNormalized = [x, y, width, height].every(value => value >= 0 && value <= 1);
    if (usesNormalized && width <= 1 && height <= 1) {
        x *= imageWidth;
        y *= imageHeight;
        width *= imageWidth;
        height *= imageHeight;
    }

    width = Math.max(8, Math.round(width));
    height = Math.max(8, Math.round(height));
    x = Math.round(x);
    y = Math.round(y);

    return {
        x: clamp(x, 0, imageWidth - 1),
        y: clamp(y, 0, imageHeight - 1),
        width: clamp(width, 1, imageWidth - x),
        height: clamp(height, 1, imageHeight - y)
    };
}

function classifyRgb(r, g, b) {
    const max = Math.max(r, g, b);
    if (max < 80) return "unknown";

    const blueScore = b - Math.max(r, g);
    const goldScore = ((r + g) / 2) - b;
    const blueDominant = b >= r && b >= g * 0.9;

    if (goldScore >= 18) return "own";
    if (blueScore >= 12 && blueDominant) return "opponent";
    return "unknown";
}

function samplePoints(box, imageWidth, imageHeight) {
    const cx = box.x + box.width / 2;
    const cy = box.y + box.height / 2;
    const left = clamp(box.x - 8, 0, imageWidth - 1);
    const right = clamp(box.x + box.width + 8, 0, imageWidth - 1);
    const above = clamp(box.y - 6, 0, imageHeight - 1);
    const below = clamp(box.y + box.height + 6, 0, imageHeight - 1);

    return [
        { x: left, y: cy },
        { x: right, y: cy },
        { x: cx, y: above },
        { x: cx, y: below },
        { x: clamp(box.x + Math.round(box.width * 0.2), 0, imageWidth - 1), y: cy }
    ];
}

function summarizeSamples(samples) {
    const votes = { own: 0, opponent: 0, unknown: 0 };
    const rgbSamples = [];

    for (const sample of samples) {
        votes[sample.label] += 1;
        rgbSamples.push(sample.rgb);
    }

    const confidentOwn = votes.own >= 2 && votes.own > votes.opponent;
    const confidentOpponent = votes.opponent >= 2 && votes.opponent > votes.own;
    const teamType = confidentOwn ? "own" : confidentOpponent ? "opponent" : "unknown";
    const average = rgbSamples.reduce((acc, rgb) => ({
        r: acc.r + rgb.r,
        g: acc.g + rgb.g,
        b: acc.b + rgb.b
    }), { r: 0, g: 0, b: 0 });
    const count = Math.max(1, rgbSamples.length);

    return {
        teamType,
        teamColor: teamType === "own" ? OWN_COLOR : teamType === "opponent" ? OPPONENT_COLOR : "",
        backgroundRgb: {
            r: Math.round(average.r / count),
            g: Math.round(average.g / count),
            b: Math.round(average.b / count)
        },
        backgroundSamples: rgbSamples,
        backgroundVotes: votes
    };
}

async function loadImagePixels(imagePath) {
    const sharp = require("sharp");
    const { data, info } = await sharp(imagePath)
        .ensureAlpha()
        .raw()
        .toBuffer({ resolveWithObject: true });

    return {
        data,
        width: info.width,
        height: info.height,
        channels: info.channels
    };
}

function readPixel(pixels, x, y) {
    const { data, width, channels } = pixels;
    const idx = (y * width + x) * channels;
    return {
        r: data[idx],
        g: data[idx + 1],
        b: data[idx + 2]
    };
}

async function classifyNameBox(imagePath, nameBox) {
    if (!imagePath || !fs.existsSync(imagePath) || !nameBox) return null;

    const pixels = await loadImagePixels(imagePath);
    const samples = samplePoints(nameBox, pixels.width, pixels.height).map(point => {
        const rgb = readPixel(pixels, point.x, point.y);
        return {
            ...point,
            rgb,
            label: classifyRgb(rgb.r, rgb.g, rgb.b)
        };
    });

    return summarizeSamples(samples);
}

function imagePathForRow(row, ocrResults) {
    const index = Number(row.sourceImageIndex || row.imageIndex || row.sourceImage || 0);
    const result = Number.isInteger(index) && index > 0 ? ocrResults[index - 1] : ocrResults[0];
    return result?.preparedPath || result?.imagePath || "";
}

function imageSizeForRow(row, ocrResults) {
    const index = Number(row.sourceImageIndex || row.imageIndex || row.sourceImage || 0);
    const result = Number.isInteger(index) && index > 0 ? ocrResults[index - 1] : ocrResults[0];
    return {
        width: Number(result?.imageWidth) || 0,
        height: Number(result?.imageHeight) || 0
    };
}

async function classifyPlayersByRowBackground(ocrResults, players) {
    if (!Array.isArray(players) || !players.length || !Array.isArray(ocrResults) || !ocrResults.length) {
        return players;
    }

    const cache = new Map();
    const classified = [];

    for (const player of players) {
        const imagePath = imagePathForRow(player, ocrResults);
        const { width, height } = imageSizeForRow(player, ocrResults);
        const nameBox = normalizeNameBox(player, width, height);

        if (!imagePath || !nameBox) {
            classified.push(player);
            continue;
        }

        const cacheKey = `${imagePath}:${nameBox.x}:${nameBox.y}:${nameBox.width}:${nameBox.height}`;
        let background = cache.get(cacheKey);
        if (!background) {
            background = await classifyNameBox(imagePath, nameBox);
            cache.set(cacheKey, background);
        }

        if (!background || background.teamType === "unknown") {
            classified.push({
                ...player,
                nameBox,
                backgroundTeamType: "unknown",
                backgroundClassification: background || null
            });
            continue;
        }

        classified.push({
            ...player,
            nameBox,
            backgroundTeamType: background.teamType,
            backgroundRgb: background.backgroundRgb,
            backgroundVotes: background.backgroundVotes,
            teamType: background.teamType,
            teamColor: background.teamColor,
            classificationSource: `row-background:rgb(${background.backgroundRgb.r},${background.backgroundRgb.g},${background.backgroundRgb.b})`,
            classificationReason: `Row background classified as ${background.teamType} from sampled pixels.`
        });
    }

    return classified;
}

async function annotateOcrImageSizes(ocrResults) {
    const sharp = require("sharp");
    const annotated = [];

    for (const result of ocrResults) {
        const imagePath = result?.preparedPath || result?.imagePath;
        if (!imagePath || !fs.existsSync(imagePath)) {
            annotated.push(result);
            continue;
        }

        const metadata = await sharp(imagePath).metadata();
        annotated.push({
            ...result,
            imageWidth: metadata.width || 0,
            imageHeight: metadata.height || 0
        });
    }

    return annotated;
}

module.exports = {
    OWN_COLOR,
    OPPONENT_COLOR,
    annotateOcrImageSizes,
    classifyNameBox,
    classifyPlayersByRowBackground,
    classifyRgb,
    normalizeNameBox
};
