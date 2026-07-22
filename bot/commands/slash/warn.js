const { SlashCommandBuilder, PermissionFlagsBits } = require("discord.js");
const { addWarning } = require("../../utils/warningStore");

module.exports = {
  data: new SlashCommandBuilder()
    .setName("warn")
    .setDescription("Warn a user and log it in the database.")
    .addUserOption(option =>
      option.setName("user")
        .setDescription("User to warn")
        .setRequired(true))
    .addStringOption(option =>
      option.setName("reason")
        .setDescription("Reason for the warning")
        .setRequired(true))
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageMessages),

  async execute(interaction) {
    const user = interaction.options.getUser("user");
    const reason = interaction.options.getString("reason") || "No reason provided.";

    try {
      await addWarning({ userId: user.id, guildId: interaction.guild.id, reason });

      return interaction.reply({ content: `✅ **${user.tag}** has been warned for: **${reason}**`, ephemeral: false });
    } catch (error) {
      console.error(error);
      return interaction.reply({ content: "❌ An error occurred while adding the warning.", ephemeral: true });
    }
  },
};
