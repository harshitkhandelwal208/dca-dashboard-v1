const { PermissionsBitField } = require("discord.js");
const { clearWarnings } = require("../../utils/warningStore");

module.exports = {
  name: "clearwarns",
  description: "Clears all warnings for a user.",
  async execute(message, args) {
    if (!message.member.permissions.has(PermissionsBitField.Flags.ManageMessages)) {
      return message.reply("You do not have permission to use this command.");
    }

    const user = message.mentions.users.first();
    if (!user) {
      return message.reply("Please mention a user to clear their warnings.");
    }

    try {
      const removed = await clearWarnings(user.id, message.guild.id);

      if (removed === 0) {
        return message.reply(`**${user.tag}** has no warnings to clear.`);
      }

      message.reply(`Cleared all warnings for **${user.tag}**.`);
    } catch (error) {
      console.error(error);
      message.reply("An error occurred while clearing warnings.");
    }
  },
};
