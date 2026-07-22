const { PermissionsBitField } = require("discord.js");
const { listWarnings } = require("../../utils/warningStore");

module.exports = {
  name: "warnings",
  description: "Displays all warnings for a user.",
  async execute(message, args) {
    if (!message.member.permissions.has(PermissionsBitField.Flags.ManageMessages)) {
      return message.reply("You do not have permission to use this command.");
    }

    const user = message.mentions.users.first();
    if (!user) {
      return message.reply("Please mention a user to check their warnings.");
    }

    try {
      const warnings = await listWarnings(user.id, message.guild.id);

      if (warnings.length === 0) {
        return message.reply(`**${user.tag}** has no warnings.`);
      }

      const warningList = warnings.map((warning, index) => `**${index + 1}.** ${warning.reason}`).join("\n");
      message.reply(`Warnings for **${user.tag}**:\n${warningList}`);
    } catch (error) {
      console.error(error);
      message.reply("An error occurred while fetching warnings.");
    }
  },
};
