const { PermissionsBitField } = require("discord.js");
const { addWarning } = require("../../utils/warningStore");

module.exports = {
  name: "warn",
  description: "Warns a user and records it in the database.",
  async execute(message, args) {
    if (!message.member.permissions.has(PermissionsBitField.Flags.ManageMessages)) {
      return message.reply("You do not have permission to use this command.");
    }

    const user = message.mentions.users.first();
    if (!user) {
      return message.reply("Please mention a user to warn.");
    }

    const reason = args.slice(1).join(" ") || "No reason provided.";

    try {
      await addWarning({ userId: user.id, guildId: message.guild.id, reason });

      message.reply(`**${user.tag}** has been warned for: **${reason}**`);
    } catch (error) {
      console.error(error);
      message.reply("An error occurred while adding the warning.");
    }
  },
};
